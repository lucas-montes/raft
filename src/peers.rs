use std::{
    net::SocketAddr,
    ops::{Deref, DerefMut},
    time::Duration,
};

use tokio::sync::mpsc::{Receiver, Sender};

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

    pub fn new(   id: impl Into<NodeId>,
    addr: impl Into<SocketAddr>,
    client: raft::Client,)->Self{
        Self { id: id.into(), addr:addr.into(), client }
    }

    pub async fn from_addr(addr: SocketAddr) -> Self {
        let client = Self::connect(addr).await;
        Self {
            id: NodeId::new(addr),
            addr,
            client,
        }
    }

    pub async fn reconnect(&mut self) {
        self.client = Self::connect(self.addr).await;
    }

    async fn connect(addr: SocketAddr) -> raft::Client {
        let mut counter = 0;
        tracing::info!(action = "adding peer", peer = addr.to_string());
        loop {
            match create_client(&addr).await {
                Ok(client) => {
                    tracing::info!(action = "peer connected", peer = addr.to_string());
                    break client;
                }
                Err(_err) => {
                    counter += 1;
                    if counter % 5 == 0 {
                        tracing::error!(
                            attempt = counter,
                            peer = addr.to_string(),
                            "failed to connect to peer"
                        );
                    }
                    tokio::time::sleep(Duration::from_millis(counter * 10)).await;
                }
            }
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct Peers(Vec<Peer>);

impl Peers {
    pub fn disconnect(&mut self, index: usize) {
        //TODO: add some awaking and background task mechanism to reconnect nodes
        let _peer = self.0.remove(index);
    }

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

pub struct PeersDisconnected(Vec<PeerDisconnected>);

impl PeersDisconnected {
    pub fn new(peers: impl Iterator<Item = Peer>) -> Self {
        Self(peers.map(PeerDisconnected::from).collect())
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

pub struct PeersReconnectionTask {
    rx: Receiver<PeersDisconnected>,
    tx: Sender<NewPeer>,
}

impl PeersReconnectionTask {
    pub fn new(rx: Receiver<PeersDisconnected>, tx: Sender<NewPeer>) -> Self {
        Self { rx, tx }
    }

    pub async fn run(mut self) {
        loop {
            tokio::select! {
                Some(rpc) = self.rx.recv() => {
                    todo!()
                }
                _ = async {} => {
                    tokio::task::yield_now().await;
                }
            }
        }
    }
}
