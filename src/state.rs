use std::{
    cmp::Ordering,
    fmt::Debug,
    io::BufWriter,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    str::FromStr,
    time::Duration,
};

use crate::{
    client::create_client,
    consensus::Consensus,
    raft_capnp::{self, raft},
    storage::{LogEntries, LogsInformation},
};

#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub enum Role {
    Leader,
    Candidate,
    #[default]
    Follower,
}

#[derive(Clone)]
pub struct Peer {
    id: NodeId,
    addr: SocketAddr,
    client: raft::Client,
}

impl Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer {{ addr: {:?} }}", self.addr)
    }
}

impl Peer {
    pub fn client(&self) -> raft::Client {
        self.client.clone()
    }

    pub async fn new(addr: SocketAddr) -> Self {
        let client = Self::connect(addr).await;
        Self {
            id: NodeId(addr),
            addr,
            client,
        }
    }

    pub async fn reconnect(&mut self) {
        self.client = Self::connect(self.addr).await;
    }

    async fn connect(addr: SocketAddr) -> raft::Client {
        tokio::task::spawn_local(async move {
            let mut counter = 0;
            tracing::info!(action = "adding peer", peer = addr.to_string());
            loop {
                match create_client(&addr).await {
                    Ok(client) => {
                        tracing::info!(action = "peer connected", peer = addr.to_string());
                        break client;
                    }
                    Err(err) => {
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
        })
        .await
        .expect("connecting to peer failed")
    }
}

#[derive(Default, Debug, Clone)]
pub struct Peers {
    connected: Vec<Peer>,
    disconnected: Vec<Peer>,
}

impl Peers {
    pub fn disconnect(&mut self, index: usize) {
        //TODO: add some awaking and background task mechanism to reconnect nodes
        let peer = self.connected.remove(index);
        self.disconnected.push(peer);
    }

    pub fn total_connected(&self) -> usize {
        //NOTE: we add one to the number of nodes to count ourself
        self.connected.len() + 1
    }
}

impl Deref for Peers {
    type Target = Vec<Peer>;

    fn deref(&self) -> &Self::Target {
        &self.connected
    }
}

impl DerefMut for Peers {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.connected
    }
}

#[derive(Debug, Clone, Eq, Copy)]
pub struct NodeId(SocketAddr);

impl NodeId {
    pub fn new(addr: SocketAddr) -> Self {
        Self(addr)
    }

    pub fn addr(&self) -> SocketAddr {
        self.0
    }
}

impl Default for NodeId {
    fn default() -> Self {
        Self(SocketAddr::from_str("127.0.0.1:4000").unwrap())
    }
}

impl PartialEq for NodeId {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq(&other.0)
    }
}

impl PartialOrd for NodeId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NodeId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

//NOTE: this needs to be persisted before responding to RPCs
#[derive(Default, Debug)]
pub struct HardState {
    current_term: u64,
    voted_for: Option<NodeId>,
    log_entries: LogEntries,
    file_path: String,
}

impl HardState {
    fn new(file_path: String) -> Self {
        Self {
            file_path,
            ..Default::default()
        }
    }

    async fn save_to_disk(&self) -> Result<(), String> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&self.file_path)
            .expect("failed to open hard state file");
        let writer = BufWriter::new(file);
        let mut message = capnp::message::Builder::new_default();

        {
            let mut hs_builder = message.init_root::<raft_capnp::hard_state::Builder>();
            hs_builder.set_current_term(self.current_term);
            let mut vote = hs_builder.reborrow().get_voted_for();

            match self.voted_for {
                None => {
                    vote.set_none(());
                }
                Some(addr) => {
                    let text = addr.0.to_string();
                    vote.set_node_id(&text);
                }
            }

            // log entries
            let mut entries = hs_builder.init_log_entries(self.log_entries.len() as u32);
            for (i, entry) in self.log_entries.iter().enumerate() {
                let mut e = entries.reborrow().get(i as u32);
                e.set_index(entry.index());
                e.set_term(entry.term());
                e.set_command(entry.command());
            }
        }

        capnp::serialize_packed::write_message(writer, &message).map_err(|err| {
            tracing::error!("failed to write hard state to disk: {}", err);
            err.to_string()
        })
    }
}

#[derive(Default, Debug)]
pub struct SoftState {
    commit_index: u64,
    last_applied: u64,
}

#[derive(Default, Debug)]
pub struct LeaderState {
    match_index: Vec<(NodeId, u64)>,
    next_index: Vec<(NodeId, u64)>,
}

#[derive(Default, Debug)]
pub struct State {
    id: NodeId,
    role: Role,
    leader: Option<NodeId>,
    leader_state: LeaderState,
    hard_state: HardState,
    soft_state: SoftState,
    peers: Peers,
}

impl State {
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            hard_state: HardState::new(format!("data/state/{}", id.addr())),
            ..Default::default()
        }
    }
    pub fn role(&self) -> Role {
        self.role
    }

    pub async fn add_peer(&mut self, addr: SocketAddr) {
        self.peers.push(Peer::new(addr).await);
    }
}

impl Consensus for State {
    fn id(&self) -> &NodeId {
        &self.id
    }

    fn peers(&self) -> &Peers {
        &self.peers
    }
    fn update_commit_index(&mut self, commit_index: u64) {
        self.soft_state.commit_index = commit_index;
    }

    fn log_entries(&mut self) -> &mut LogEntries {
        &mut self.hard_state.log_entries
    }

    fn become_candidate(&mut self) {
        self.role = Role::Candidate;
        self.hard_state.current_term += 1;
        self.hard_state.voted_for = Some(self.id);
        // self.soft_state.commit_index = 0;
        self.leader = None;
        //     self.heartbeat_latency = random_range(1.0..2.9);
    }

    fn become_follower(&mut self, leader_id: Option<SocketAddr>, new_term: u64) {
        self.hard_state.current_term = new_term;
        self.hard_state.voted_for = None;
        // self.soft_state.commit_index = 0;
        self.leader = leader_id.map(NodeId::new);
    }

    fn become_leader(&mut self) {
        self.role = Role::Leader;
        self.leader = Some(self.id);
        // self.leader_state.match_index = vec![(self.id, 0)];
        // self.leader_state.next_index = vec![(self.id, 0)];
    }

    fn leader(&self) -> Option<SocketAddr> {
        self.leader.as_ref().map(|node| node.addr())
    }

    fn vote_for(&mut self, node: NodeId) {
        tracing::info!(action = "voting for", node = node.addr().to_string());
        self.hard_state.voted_for = Some(node);
    }

    fn voted_for(&self) -> Option<&NodeId> {
        self.hard_state.voted_for.as_ref()
    }

    fn current_term(&self) -> u64 {
        self.hard_state.current_term
    }

    fn commit_index(&self) -> u64 {
        self.soft_state.commit_index
    }

    fn last_log_info(&self) -> LogsInformation {
        self.hard_state.log_entries.last_log_info()
    }

    async fn restart_peer(&mut self, peer_index: usize) {
        self.peers[peer_index].reconnect().await
    }

    async fn commit_hard_state(&self) {
        self.hard_state
            .save_to_disk()
            .await
            .expect("failed to save hard state");
    }
}
