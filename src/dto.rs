use crate::raft_capnp;



pub struct RequestVoteResponse {
    term: u64,
    vote_granted: bool,
}

impl RequestVoteResponse {
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

impl<'a> From <raft_capnp::request_vote_response::Reader<'a>> for RequestVoteResponse {
    fn from(resp: raft_capnp::request_vote_response::Reader<'a>) -> Self {
        RequestVoteResponse {
            term: resp.get_term(),
            vote_granted: resp.get_vote_granted(),
        }
    }
}




pub struct AppendEntriesResponse {
    term: u64,
    success: bool,
}

impl AppendEntriesResponse {
    pub fn successful(term: u64) -> Self {
        Self {
            term,
            success: true,
        }
    }
    pub fn failed(term: u64) -> Self {
        Self {
            term,
            success: false,
        }
    }

    pub fn term(&self) -> u64 {
        self.term
    }
    pub fn success(&self) -> bool {
        self.success
    }
}


impl<'a> From <raft_capnp::append_entries_response::Reader<'a>> for AppendEntriesResponse {
    fn from(resp: raft_capnp::append_entries_response::Reader<'a>) -> Self {
        AppendEntriesResponse {
            term: resp.get_term(),
            success: resp.get_success(),
        }
    }
}
