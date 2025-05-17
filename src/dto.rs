use tokio::sync::oneshot::{self, Receiver};

use crate::{consensus::AppendEntriesResult, raft_capnp, storage::LogEntry};

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
    pub msg: M,
    pub sender: oneshot::Sender<R>,
}

pub struct VoteRequest {
    term: u64,
    candidate_id: String,
    last_log_index: u64,
    last_log_term: u64,
}

pub struct AppendEntriesRequest {
    term: u64,
    leader_id: String,
    prev_log_index: u64,
    prev_log_term: u64,
    entries: Vec<LogEntry>,
    leader_commit: u64,
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
