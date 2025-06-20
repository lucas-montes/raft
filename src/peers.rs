use std::{
    net::SocketAddr,
    ops::{Deref, DerefMut},
    time::Duration,
};

use tokio::{
    sync::mpsc::{Receiver, Sender},
    task::JoinSet,
};

use crate::{client::create_client, raft_capnp::raft, state::NodeId};

pub trait PeersManagement {
    fn add_peer(&mut self, peer: impl Into<Peer>);
    fn remove_peer(&mut self, index: usize) -> Peer;
    fn peers(&self) -> &Peers;
}

#[derive(Clone)]
pub struct Peer {
    id: NodeId,
    addr: SocketAddr,
    client: raft::Client,
}

impl std::fmt::Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer {{ addr: {:?} }}", self.addr)
    }
}

impl Peer {
    pub fn client(&self) -> raft::Client {
        self.client.clone()
    }

    pub fn new(id: impl Into<NodeId>, addr: impl Into<SocketAddr>, client: raft::Client) -> Self {
        Self {
            id: id.into(),
            addr: addr.into(),
            client,
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct Peers(Vec<Peer>);

impl Peers {
    pub fn total_connected(&self) -> usize {
        self.0.len()
    }
}

impl Deref for Peers {
    type Target = Vec<Peer>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Peers {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct NewPeer(Peer);

impl From<Peer> for NewPeer {
    fn from(value: Peer) -> Self {
        Self(value)
    }
}

impl From<NewPeer> for Peer {
    fn from(val: NewPeer) -> Self {
        val.0
    }
}

pub struct PeerDisconnected {
    id: NodeId,
    addr: SocketAddr,
}

impl PeerDisconnected {
    pub fn new(id: NodeId, addr: SocketAddr) -> Self {
        Self { id, addr }
    }
}

impl From<Peer> for PeerDisconnected {
    fn from(value: Peer) -> Self {
        Self {
            id: value.id,
            addr: value.addr,
        }
    }
}

pub struct PeersDisconnected(Vec<PeerDisconnected>);

impl PeersDisconnected {
    pub fn new<T: Into<PeerDisconnected>>(peers: impl Iterator<Item = T>) -> Self
    where
        PeerDisconnected: From<T>,
    {
        Self(peers.map(PeerDisconnected::from).collect())
    }
}

pub struct PeersReconnectionTask {
    rx: Receiver<PeersDisconnected>,
    tx: Sender<NewPeer>,
    initial_backoff_ms: u64,
    max_backoff_ms: u64,
}

impl PeersReconnectionTask {
    pub fn new(
        rx: Receiver<PeersDisconnected>,
        tx: Sender<NewPeer>,
        initial_backoff_ms: u64,
        max_backoff_ms: u64,
    ) -> Self {
        Self {
            rx,
            tx,
            initial_backoff_ms,
            max_backoff_ms,
        }
    }

    pub async fn run(mut self) {
        let mut reconnection_tasks: JoinSet<NewPeer> = JoinSet::new();

        loop {
            tokio::select! {
                // Handle new disconnected peers
                Some(peers_disconnected) = self.rx.recv() => {
                    for peer_disconnected in peers_disconnected.0 {
                        reconnection_tasks.spawn_local(Self::reconnect_peer(
                            self.initial_backoff_ms,
                            self.max_backoff_ms,
                            peer_disconnected));
                    }
                }

                // Handle completed reconnections
                Some(reconnection_result) = reconnection_tasks.join_next() => {
                    match reconnection_result {
                        Ok(new_peer) => {
                            // Send the reconnected peer back to the main node
                            if let Err(e) = self.tx.send(new_peer).await {
                                tracing::error!(action = "failed_to_send_reconnected_peer", error = ?e);
                            }
                        }
                        Err(join_error) => {
                            tracing::error!(action = "reconnection_task_failed", error = ?join_error);
                        }
                    }
                }

                // Yield control when no work is available
                else => {
                    tokio::task::yield_now().await;
                }
            }
        }
    }

    async fn reconnect_peer(
        mut backoff_ms: u64,
        max_backoff_ms: u64,
        peer_disconnected: PeerDisconnected,
    ) -> NewPeer {
        let addr = peer_disconnected.addr;
        loop {
            match create_client(&addr).await {
                Ok(client) => {
                    tracing::info!(action = "peerReconnectedSuccessfully", peer = %addr);
                    let peer = Peer::new(peer_disconnected.id, addr, client);
                    return NewPeer::from(peer);
                }
                Err(e) => {
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    backoff_ms = std::cmp::min(backoff_ms * 2, max_backoff_ms);
                }
            }
        }
    }
}
