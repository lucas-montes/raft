use std::net::SocketAddr;

use tokio::sync::oneshot::{self, Receiver};

use crate::{
    consensus::AppendEntriesResult,
    raft_capnp::{self},
    storage::LogEntry,
};

#[derive(Debug)]
pub struct VoteResponse {
    term: u64,
    vote_granted: bool,
}

impl VoteResponse {
    pub fn granted(term: u64) -> Self {
        Self {
            term,
            vote_granted: true,
        }
    }
    pub fn not_granted(term: u64) -> Self {
        Self {
            term,
            vote_granted: false,
        }
    }

    pub fn term(&self) -> u64 {
        self.term
    }
    pub fn vote_granted(&self) -> bool {
        self.vote_granted
    }
}

impl<'a> From<raft_capnp::vote_response::Reader<'a>> for VoteResponse {
    fn from(resp: raft_capnp::vote_response::Reader<'a>) -> Self {
        VoteResponse {
            term: resp.get_term(),
            vote_granted: resp.get_vote_granted(),
        }
    }
}

//TODO: this should probably be a regular result
pub enum AppendEntriesResponse {
    Ok,
    Err(u64),
}

impl TryFrom<raft_capnp::append_entries_response::Reader<'_>> for AppendEntriesResponse {
    type Error = capnp::Error;

    fn try_from(
        resp: raft_capnp::append_entries_response::Reader<'_>,
    ) -> Result<Self, Self::Error> {
        match resp.which()? {
            raft_capnp::append_entries_response::Ok(()) => Ok(AppendEntriesResponse::Ok),
            raft_capnp::append_entries_response::Err(err) => Ok(AppendEntriesResponse::Err(err)),
        }
    }
}

#[derive(Debug)]
pub struct Msg<M, R> {
    pub msg: M,
    pub sender: oneshot::Sender<R>,
}

#[derive(Debug)]
pub struct VoteRequest {
    term: u64,
    candidate_id: String,
    last_log_index: u64,
    last_log_term: u64,
}
impl VoteRequest {
    pub fn term(&self) -> u64 {
        self.term
    }
    pub fn candidate_id(&self) -> &str {
        &self.candidate_id
    }
    pub fn last_log_index(&self) -> u64 {
        self.last_log_index
    }
    pub fn last_log_term(&self) -> u64 {
        self.last_log_term
    }
}

#[derive(Debug)]
pub struct AppendEntriesRequest {
    pub term: u64,
    pub leader_id: String,
    pub prev_log_index: u64,
    pub prev_log_term: u64,
    pub entries: Vec<LogEntry>,
    pub leader_commit: u64,
}
#[derive(Debug)]
pub struct Entry {
    pub id: String,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub struct Query {
    id: String,
    filter: Option<String>,
}
impl Query {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }
}

#[derive(Debug)]
pub enum Operation {
    Create(Vec<u8>),
    Update(String, Vec<u8>),
    Delete(String),
}

#[derive(Debug)]
pub enum CommandMsg {
    GetLeader(Msg<(), Option<SocketAddr>>),
    Read(Msg<Query, Entry>),
    Modify(Msg<Operation, Entry>),
}

impl CommandMsg {
    pub fn delete(id: String) -> (Self, Receiver<Entry>) {
        let (tx, rx) = oneshot::channel();
        let msg = Self::Modify(Msg {
            msg: Operation::Delete(id),
            sender: tx,
        });
        (msg, rx)
    }
    pub fn create(data: Vec<u8>) -> (Self, Receiver<Entry>) {
        let (tx, rx) = oneshot::channel();
        let msg = Self::Modify(Msg {
            msg: Operation::Create(data),
            sender: tx,
        });
        (msg, rx)
    }
    pub fn update(id: String, data: Vec<u8>) -> (Self, Receiver<Entry>) {
        let (tx, rx) = oneshot::channel();
        let msg = Self::Modify(Msg {
            msg: Operation::Update(id, data),
            sender: tx,
        });
        (msg, rx)
    }
    pub fn get_leader() -> (Self, Receiver<Option<SocketAddr>>) {
        let (tx, rx) = oneshot::channel();
        let msg = Self::GetLeader(Msg {
            msg: (),
            sender: tx,
        });
        (msg, rx)
    }
}

#[derive(Debug)]
pub enum RaftMsg {
    AppendEntries(Msg<AppendEntriesRequest, AppendEntriesResult>),
    Vote(Msg<VoteRequest, VoteResponse>),
}

impl RaftMsg {
    pub fn request_append_entries(
        term: u64,
        leader_id: String,
        prev_log_index: u64,
        prev_log_term: u64,
        entries: Vec<LogEntry>,
        leader_commit: u64,
    ) -> (Self, Receiver<AppendEntriesResult>) {
        let (tx, rx) = oneshot::channel();

        let req = AppendEntriesRequest {
            term,
            leader_id,
            prev_log_index,
            prev_log_term,
            entries,
            leader_commit,
        };
        let msg = Self::AppendEntries(Msg {
            msg: req,
            sender: tx,
        });

        (msg, rx)
    }

    pub fn request_vote(
        term: u64,
        candidate_id: String,
        last_log_index: u64,
        last_log_term: u64,
    ) -> (Self, Receiver<VoteResponse>) {
        let (tx, rx) = oneshot::channel();

        let req = VoteRequest {
            term,
            candidate_id,
            last_log_index,
            last_log_term,
        };
        let msg = Self::Vote(Msg {
            msg: req,
            sender: tx,
        });

        (msg, rx)
    }
}
