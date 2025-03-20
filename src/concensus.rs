use std::{
    cell::RefCell,
    cmp::Ordering,
    fmt::Debug,
    net::SocketAddr,
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Debug, Default, Copy, Clone, PartialEq)]
pub enum Role {
    Leader {
        next_index: u64,
        match_index: u64,
    },
    Candidate,
    #[default]
    Follower,
}

#[derive(Debug, Eq)]
struct LogEntry {
    index: u64,
    term: u64,
    msg: String,
}

impl PartialEq for LogEntry {
    fn eq(&self, other: &Self) -> bool {
        self.index.eq(&other.index) && self.term.eq(&other.term)
    }
}

impl PartialOrd for LogEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Implement Ord
impl Ord for LogEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.term.cmp(&other.term) {
            Ordering::Equal => {
                // If terms are equal, compare index
                self.index.cmp(&other.index)
            }
            ordering => ordering,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Node(Rc<RefCell<State>>);

impl Node {
    pub fn last_log_info(&self) -> (u64, u64) {
        let state = self.0.borrow();
        let last_log = state.log_entries.last();
        match last_log {
            Some(last_log) => (last_log.term, last_log.index),
            None => (0, 0),
        }
    }

    pub fn voted_for(&self) -> Option<SocketAddr> {
        self.0.borrow().voted_for
    }

    pub fn role(&self) -> Role {
        self.0.borrow().role
    }

    pub fn addr(&self) -> SocketAddr {
        self.0.borrow().addr
    }

    pub fn current_term(&self) -> u64 {
        self.0.borrow().current_term
    }

    pub fn new(addr: SocketAddr) -> Self {
        Self(Rc::new(RefCell::new(State {
            last_heartbeat: None,
            current_term: 0,
            voted_for: None,
            log_entries: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            role: Role::Follower,
            addr,
        })))
    }
    pub fn check_election(&mut self, election_timeout: Duration) {
        self.0.borrow_mut().check_election(election_timeout)
    }

    pub fn make_leader(&mut self) {
        self.0.borrow_mut().become_leader()
    }
}

struct State {
    last_heartbeat: Option<Instant>,
    current_term: u64,
    voted_for: Option<SocketAddr>,
    log_entries: Vec<LogEntry>,
    commit_index: u64,
    last_applied: u64,
    role: Role,
    addr: SocketAddr,
}

impl Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "role: {:?} current_term: {:?} voted_for: {:?}",
            self.role, self.current_term, self.voted_for
        )
    }
}
impl State {
    fn start_election(&mut self) {
        println!("elections noooow");
        self.role = Role::Candidate;
        self.current_term += 1;
        self.voted_for = Some(self.addr);
    }

    fn become_leader(&mut self) {
        println!("im the leader now");
        let last_log = self.log_entries.last().map(|l| l.index).unwrap_or_default();
        self.role = Role::Leader {
            next_index: last_log + 1,
            match_index: last_log,
        };
    }

    fn check_election(&mut self, election_timeout: Duration) {
        if self
            .last_heartbeat
            .is_none_or(|t| t.elapsed() >= election_timeout)
            && self.role == Role::Follower
        {
            self.start_election()
        }
    }
}
