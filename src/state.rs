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
    client::{append_entries, create_client, vote},
    consensus::Consensus,
    dto::{CommandMsg, Entry, Operation, RaftMsg},
    raft_capnp::raft,
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

    async fn new(addr: SocketAddr) -> Self {
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
            tracing::info!("trying to connect");
            loop {
                match create_client(&addr).await {
                    Ok(client) => {
                        tracing::info!("connected");
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
        self.leader.as_ref().map(|node| node.addr())
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

    fn last_log_info(&self) -> LogsInformation {
        self.hard_state.log_entries.last_log_info()
    }
    async fn restart_peer(&mut self, peer_index: usize) {
        self.peers[peer_index].reconnect().await
    }
}

#[derive(Debug)]
pub struct Node {
    state: State,
    heartbeat_interval: f64,
    election_timeout: f64,
    raft_channel: Receiver<RaftMsg>,
    commands_channel: Receiver<CommandMsg>,
}

impl Node {
    pub fn new(
        state: State,
        heartbeat_interval: f64,
        election_timeout: f64,
        raft_channel: Receiver<RaftMsg>,
        commands_channel: Receiver<CommandMsg>,
    ) -> Self {
        Self {
            state,
            raft_channel,
            heartbeat_interval,
            election_timeout,
            commands_channel,
        }
    }

    pub async fn add_peer(&mut self, addr: SocketAddr) {
        self.state.peers.push(Peer::new(addr).await);
    }

    pub async fn run(mut self) {
        let heartbeat_dur = Duration::from_secs_f64(self.heartbeat_interval);
        let election_dur = Duration::from_secs_f64(self.election_timeout);
        let mut heartbeat_interval = interval(heartbeat_dur);
        let mut election_timeout = Box::pin(sleep(election_dur));
        let mut raft_channel = self.raft_channel;
        let mut commands_channel = self.commands_channel;

        loop {
            tokio::select! {
                //  Incoming RPCs from external users
                Some(rpc) = commands_channel.recv() => {
                    match rpc {
                        CommandMsg::GetLeader(req) => {
                            let resp = self.state.leader();
                            let sender = req.sender;
                            if let Err(_) = sender.send(resp){
                                tracing::error!("Failed to send response in channel for get leader");
                            }
                        }
                        CommandMsg::Read(req) => {
                            // Handle read command
                        }
                        CommandMsg::Modify(req) => {
                           match req.msg {
                                Operation::Create(data) => {
                                    let resp = self.state.log_entries().create(data.clone()).await;
                                    let sender = req.sender;
                                    if let Err(_) = sender.send(Entry {
                                        id: "hey".to_string(),
                                        data
                                    }){
                                        tracing::error!("Failed to send response in channel for create");
                                    }
                                }
                                Operation::Update(id, data) => {
                                    // Handle update command
                                }
                                Operation::Delete(id) => {
                                    // Handle delete command
                                }
                            }
                        }
                    }

                }

                Some(rpc) = raft_channel.recv() => {
                    tracing::info!("electinos time resest {:?} ecause of rpc", election_timeout.deadline().elapsed());
                    election_timeout.as_mut().reset(Instant::now() + election_dur);
                    match rpc {
                        RaftMsg::AppendEntries(req) => {
                            let msg = req.msg;
                            let sender = req.sender;
                            let resp = self.state.handle_append_entries(
                                msg.term,
                                &msg.leader_id,
                                msg.prev_log_index as usize,
                                msg.prev_log_term,
                                msg.leader_commit,
                                msg.entries,
                            );
                            if let Err(_) = sender.send(resp){
                                tracing::error!("Failed to send response in channel for append entries");
                            }
                        }
                        RaftMsg::Vote(req) => {
                            let msg = req.msg;
                            let sender = req.sender;
                            let resp = self.state.handle_request_vote(
                                msg.term(),
                                msg.candidate_id(),
                                msg.last_log_index(),
                                msg.last_log_term(),
                            );
                            if let Err(_) = sender.send(resp){
                                tracing::error!("Failed to send response in channel for vote");
                            }
                        }
                    }
                }

                //TODO: maybe the following functions could be driven by the role and a trait

                //  election timeout fires → start election
                _ = &mut election_timeout, if self.state.role != Role::Leader => {
                    tracing::info!("electinos time put {:?}", election_timeout.deadline().elapsed());
                    election_timeout.as_mut().reset(Instant::now() + election_dur);
                    tracing::info!("electinos time");
                    self.state.become_candidate();
                    vote(&mut self.state).await;
                }

                //  heartbeat tick → send heartbeats if leader
                _ = heartbeat_interval.tick(), if self.state.role == Role::Leader => {
                    tracing::info!("sending heartbeats");
                    append_entries(&mut self.state, &[]).await;
                }
            }
        }
    }
}
