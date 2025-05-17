use std::{
    cmp::Ordering,
    fmt::Debug,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    str::FromStr,
    time::Duration,
};

use tokio::{
    sync::mpsc::Receiver,
    time::{interval, sleep, Instant},
};

use crate::{
    client::create_client, consensus::Consensus, dto::RaftMsg, raft_capnp::raft,
    storage::LogEntries,
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

    async fn new(addr: SocketAddr) -> Self {
        let client = Self::connect(&addr).await;
        Self {
            id: NodeId(addr),
            addr,
            client,
        }
    }

    pub async fn reconnect(&mut self) {
        self.client = Self::connect(&self.addr).await;
    }

    async fn connect(addr: &SocketAddr) -> raft::Client {
        let mut counter = 0;
        loop {
            match create_client(addr).await {
                Ok(client) => {
                    break client;
                }
                Err(err) => {
                    counter += 1;
                    tokio::time::sleep(Duration::from_secs(counter)).await;
                }
            }
        }
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

#[derive(Default, Debug, Clone)]
pub struct Peers(Vec<Peer>);

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
            ..Default::default()
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.id.0
    }

    pub fn role(&self) -> Role {
        self.role
    }
}

impl Consensus for State {
    fn peers(&self) -> &Peers {
        &self.peers
    }
    fn update_commit_index(&mut self, commit_index: u64) {
        self.soft_state.commit_index = commit_index;
    }

    fn log_entries(&self) -> &Vec<crate::storage::LogEntry> {
        // &self.hard_state.log_entries.0
        todo!()
    }

    fn become_candidate(&mut self) {
        self.role = Role::Candidate;
        self.hard_state.current_term += 1;
        self.hard_state.voted_for = Some(self.id);
        // self.soft_state.commit_index = 0;
        self.leader = None;
    }

    fn become_follower(&mut self, leader_id: Option<SocketAddr>, new_term: u64) {
        self.hard_state.current_term = new_term;
        self.hard_state.voted_for = None;
        self.soft_state.commit_index = 0;
        self.leader = leader_id.map(NodeId::new);
    }

    fn become_leader(&mut self) {
        self.role = Role::Leader;
        self.leader = Some(self.id);
        // self.leader_state.match_index = vec![(self.id, 0)];
        // self.leader_state.next_index = vec![(self.id, 0)];
    }

    fn leader(&self) -> Option<SocketAddr> {
        // self.leader.map(|id| id.addr())
        todo!()
    }

    fn vote_for(&mut self, node: NodeId) {
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

    fn last_log_info(&self) -> (u64, u64) {
        self.hard_state.log_entries.last_log_info()
    }
}

#[derive(Debug)]
pub struct Node {
    state: State,
    latency: f64,
    raft_channel: Receiver<RaftMsg>,
    commands_channel: Receiver<RaftMsg>,
}

impl Node {
    pub fn new(
        state: State,
        latency: f64,
        raft_channel: Receiver<RaftMsg>,
        commands_channel: Receiver<RaftMsg>,
    ) -> Self {
        Self {
            state,
            raft_channel,
            latency,
            commands_channel,
        }
    }

    pub async fn add_peer(&mut self, addr: SocketAddr) {
        self.state.peers.push(Peer::new(addr).await);
    }

    pub async fn run(self) {
        let dur = Duration::from_secs_f64(self.latency);
        let mut heartbeat_interval = interval(dur);
        let mut election_timeout = Box::pin(sleep(dur));
        let mut raft_channel = self.raft_channel;
        let mut commands_channel = self.commands_channel;

        loop {
            tokio::select! {
                //  Incoming RPCs from external users
                Some(rpc) = commands_channel.recv() => {

                }

                Some(rpc) = raft_channel.recv() => {
                    election_timeout.as_mut().reset(Instant::now() + dur);
                    //self.handle_rpc(rpc).await;
                    // on AppendEntries success or higher‐term RPC:
                    // reset election_timeout
                }

                //  election timeout fires → start election
                _ = &mut election_timeout, if self.state.role != Role::Leader => {
                    // self.become_candidate().await;
                    // Clear votes, increment term, vote for self, send RequestVotes...

                }

                //  heartbeat tick → send heartbeats if leader
                _ = heartbeat_interval.tick(), if self.state.role == Role::Leader => {
                    //self.send_heartbeats().await;
                }
            }
        }
    }
}
