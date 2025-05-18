use std::net::SocketAddr;

use tokio::sync::oneshot::{self, Receiver};

use crate::{consensus::AppendEntriesResult, raft_capnp::{self, raft}, storage::LogEntry};

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

pub struct Msg<M, R> {
    msg: M,
    sender: oneshot::Sender<R>,
}

impl<M, R> Msg<M, R> {
    pub fn mesage(&self) -> &M {
        &self.msg
    }

    pub fn send_response(self, response: R) {
        if let Err(_) = self.sender.send(response){
            println!("Failed to send response");
        }
    }
}



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

pub struct AppendEntriesRequest {
    term: u64,
    leader_id: String,
    prev_log_index: u64,
    prev_log_term: u64,
    entries: Vec<LogEntry>,
    leader_commit: u64,
}

impl AppendEntriesRequest {
pub fn term(&self) -> u64 {
        self.term
    }
    pub fn leader_id(&self) -> &str {
        &self.leader_id
    }
    pub fn prev_log_index(&self) -> u64 {
        self.prev_log_index
    }
    pub fn prev_log_term(&self) -> u64 {
        self.prev_log_term
    }
    pub fn entries(&self) -> &Vec<LogEntry> {
        &self.entries
    }
    pub fn leader_commit(&self) -> u64 {
        self.leader_commit
    }
}

pub enum CommandMsg{
    GetLeader(Msg<(), Option<SocketAddr>>),
    Read(Msg<AppendEntriesRequest, AppendEntriesResult>),
    Modify(Msg<AppendEntriesRequest, AppendEntriesResult>)
}

impl CommandMsg {
    pub fn get_leader() -> (Self, Receiver<Option<SocketAddr>>) {
        let (tx, rx) = oneshot::channel();
        let msg = Self::GetLeader(Msg {
            msg: (),
            sender: tx,
        });
        (msg, rx)
    }
}

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
