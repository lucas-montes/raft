use std::{
    cmp::Ordering,
    fmt::Debug,
    io::BufWriter,
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
};

use crate::{
    consensus::Consensus,
    peers::{Peer, Peers, PeersManagement},
    raft_capnp::{self},
    storage::{LogEntries, LogEntry, LogsInformation},
};

#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub enum Role {
    Leader,
    Candidate,
    #[default]
    Follower,
}

#[derive(Debug, Clone, Eq, Copy)]
pub struct NodeId(SocketAddr);

impl From<&str> for NodeId {
    fn from(addr: &str) -> Self {
        let addr = SocketAddr::from_str(addr).expect("failed to parse NodeId from string");
        Self(addr)
    }
}

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
    file_path: PathBuf,
}

impl HardState {
    fn new(file_path: PathBuf) -> Self {
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

    fn load_from_disk(&mut self) -> Result<(), String> {
        std::fs::create_dir_all(self.file_path.parent().expect("couldnt get parent of path"))
            .expect("couldnt creat parent dir for hard state");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .append(true)
            .truncate(false)
            .create(true)
            .open(&self.file_path)
            .expect("failed to open hard state file");

        let reader = std::io::BufReader::new(file);

        let message_reader = match capnp::serialize_packed::read_message(
            reader,
            capnp::message::ReaderOptions::new(),
        ) {
            Ok(message_reader) => message_reader,
            Err(err) => match err.kind {
                capnp::ErrorKind::PrematureEndOfFile => {
                    tracing::info!(action = "createHardState");
                    return Ok(());
                }
                _ => {
                    tracing::error!("failed to read hard state message: {}", err);
                    tracing::error!("failed to read hard state message: {}", err.kind);
                    return Err(err.to_string());
                }
            },
        };

        let hs_reader = message_reader
            .get_root::<raft_capnp::hard_state::Reader>()
            .map_err(|e| {
                tracing::error!("failed to get hard state root: {}", e);
                e.to_string()
            })?;

        self.current_term = hs_reader.get_current_term();
        match hs_reader.get_voted_for().which() {
            Ok(raft_capnp::hard_state::voted_for::None(())) => self.voted_for = None,
            Ok(raft_capnp::hard_state::voted_for::NodeId(node_id)) => match node_id {
                Ok(node_id) => {
                    let addr = SocketAddr::from_str(node_id.to_str().unwrap()).map_err(|e| {
                        tracing::error!("failed to parse NodeId from string: {}", e);
                        e.to_string()
                    })?;
                    self.voted_for = Some(NodeId(addr));
                }
                Err(e) => {
                    tracing::error!("failed to parse NodeId from string: {}", e);
                    return Err(e.to_string());
                }
            },

            Err(e) => {
                tracing::error!("failed to read voted_for: {}", e);
                return Err(e.to_string());
            }
        }

        let entries_reader = hs_reader.get_log_entries().map_err(|e| {
            tracing::error!("failed to get log entries: {}", e);
            e.to_string()
        })?;

        self.log_entries = entries_reader
            .iter()
            .map(|e| {
                LogEntry::new(
                    e.get_index(),
                    e.get_term(),
                    e.get_command()
                        .map_err(|err| err.to_string())
                        .expect("config loading failed")
                        .to_vec(),
                )
            })
            .collect();
        Ok(())
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
    pub fn new(id: NodeId, path: impl AsRef<Path>) -> Self {
        //TODO: no need for that later on
        let mut path = path.as_ref().to_path_buf();
        path.push(id.addr().to_string().replace(":", "_"));
        //
        let mut hard_state = HardState::new(path);
        hard_state
            .load_from_disk()
            .expect("hard config load failed");
        Self {
            id,
            hard_state,
            ..Default::default()
        }
    }
}

impl PeersManagement for State {
    fn add_peer(&mut self, peer: impl Into<Peer>) {
        self.peers.push(peer.into());
    }

    fn remove_peer(&mut self, index: usize) -> Peer {
        self.peers.remove(index)
    }

    fn peers(&self) -> &Peers {
        &self.peers
    }
}

impl Consensus for State {
    fn role(&self) -> &Role {
        &self.role
    }

    fn cluster_size(&self) -> u64 {
        //NOTE: we add one to the number of nodes to count ourself
        self.peers.total_connected() as u64 + 1
    }

    fn id(&self) -> &NodeId {
        &self.id
    }

    fn update_commit_index(&mut self, commit_index: u64) {
        self.soft_state.commit_index = commit_index;
    }

    fn log_entries(&mut self) -> &mut LogEntries {
        &mut self.hard_state.log_entries
    }

    fn become_candidate(&mut self) {
        self.role = Role::Candidate;
        tracing::info!(action = "becomeCandidate");
        self.hard_state.current_term += 1;
        self.hard_state.voted_for = Some(self.id);
        // self.soft_state.commit_index = 0;
        self.leader = None;
        //     self.heartbeat_latency = random_range(1.0..2.9);
    }

    fn become_follower(&mut self, leader_id: Option<SocketAddr>, new_term: u64) {
        self.hard_state.current_term = new_term;
        tracing::info!(action = "becomeFollower", term = new_term);
        self.hard_state.voted_for = None;
        self.role = Role::Follower;
        // self.soft_state.commit_index = 0;
        self.leader = leader_id.map(NodeId::new);
    }

    fn become_leader(&mut self) {
        self.role = Role::Leader;
        tracing::info!(
            action = "becomeLeader",
            term = self.current_term(),
            peers = self.cluster_size()
        );
        self.leader = Some(self.id);
        // self.leader_state.match_index = vec![(self.id, 0)];
        // self.leader_state.next_index = vec![(self.id, 0)];
    }

    fn leader(&self) -> Option<SocketAddr> {
        self.leader.as_ref().map(|node| node.addr())
    }

    fn vote_for(&mut self, node: NodeId) {
        tracing::info!(action = "votingFor", node = node.addr().to_string());
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

    async fn commit_hard_state(&self) {
        self.hard_state
            .save_to_disk()
            .await
            .expect("failed to save hard state");
    }
}
