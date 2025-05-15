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

impl<'a> From<raft_capnp::request_vote_response::Reader<'a>> for RequestVoteResponse {
    fn from(resp: raft_capnp::request_vote_response::Reader<'a>) -> Self {
        RequestVoteResponse {
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

    fn try_from(resp: raft_capnp::append_entries_response::Reader<'_>) -> Result<Self, Self::Error> {
        match resp.which()?{
            raft_capnp::append_entries_response::Ok(()) => {
                Ok(AppendEntriesResponse::Ok)
            }
            raft_capnp::append_entries_response::Err(err) => {
                Ok(AppendEntriesResponse::Err(err))
            }
        }
    }
}

// impl<'a> TryFrom<raft_capnp::append_entries_response::Reader<'a>> for AppendEntriesResponse {
//     fn try_from(resp: raft_capnp::append_entries_response::Reader<'a>) -> Self {
//         resp.which()
//         AppendEntriesResponse {
//             term: resp.get_term(),
//             success: resp.get_success(),
//         }
//     }
// }
