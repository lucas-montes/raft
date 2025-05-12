use std::{
    cell::RefCell, cmp::Ordering, fmt::Debug, net::SocketAddr, ops::{Deref, DerefMut}, rc::Rc, str::FromStr, time::{Duration, Instant}
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


pub struct AppendEntriesResponse{
    term: u64,
    success: bool,
}

impl AppendEntriesResponse {
    fn successful(term: u64) -> Self {
        Self { term, success: true }
    }
    fn failed(term: u64) -> Self {
        Self { term, success: false }
    }

    pub fn term(&self) -> u64 {
        self.term
    }
    pub fn success(&self) -> bool {
        self.success
    }
}

pub struct RequestVoteResponse{
    term: u64,
    vote_granted: bool,
}

impl RequestVoteResponse {
    fn granted(term: u64) -> Self {
        Self { term, vote_granted: true }
    }
    fn not_granted(term: u64) -> Self {
        Self { term, vote_granted: false }
    }

    pub fn term(&self) -> u64 {
        self.term
    }
    pub fn vote_granted(&self) -> bool {
        self.vote_granted
    }
}

#[derive(Debug, Eq)]
pub struct LogEntry {
    index: u64,
    term: u64,
    command: String,
}

impl LogEntry {
    pub fn new(index: u64, term: u64, command: String) -> Self {
        Self {
            index,
            term,
            command,
        }
    }
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

impl Ord for LogEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.index.cmp(&other.index)
    }
}

#[derive(Debug, Clone)]
pub struct Node(Rc<RefCell<State>>);

impl Node {
    pub fn handle_append_entries(
        &self,
        term: u64,
        leader_id: &str,
        prev_log_index: usize,
        prev_log_term: u64,
        leader_commit: u64,
        entries: Vec<LogEntry>,
    ) -> AppendEntriesResponse {
        self.0.borrow_mut().handle_append_entries(term, leader_id, prev_log_index, prev_log_term, leader_commit,entries)
    }


    pub fn handle_request_vote(
        &self,
        term: u64,
        candidate_id: &str,
        last_log_index: u64,
        last_log_term: u64,
    ) -> RequestVoteResponse {
        self.0
            .borrow_mut()
            .handle_request_vote(term, candidate_id, last_log_index, last_log_term)
    }

    pub fn last_log_info(&self) -> (u64, u64) {
        self.0.borrow().last_log_info()
    }

    pub fn heartbeat_latency(&self) -> f64 {
        self.0.borrow().heartbeat_latency
    }

    pub fn commit_index(&self) -> u64 {
        self.0.borrow().commit_index
    }

    pub fn voted_for(&self) -> Option<SocketAddr> {
        self.0.borrow().voted_for
    }

    pub async fn role(&self) -> Role {
        self.0.borrow().role
    }

    pub fn addr(&self) -> SocketAddr {
        self.0.borrow().addr
    }

    pub fn current_term(&self) -> u64 {
        self.0.borrow().current_term
    }

    pub fn new(addr: SocketAddr, latency: f64) -> Self {
        Self(Rc::new(RefCell::new(State::new(addr, latency))))
    }
    pub fn check_election(&mut self, election_timeout: Duration) {
        self.0.borrow_mut().check_election(election_timeout)
    }

    pub fn make_leader(&mut self) {
        self.0.borrow_mut().become_leader()
    }

    pub fn update_heartbeat(&mut self) {
        println!("beating");
        self.0.borrow_mut().last_heartbeat = Some(Instant::now());
    }
}


#[derive(Debug, Default)]
pub struct LogEntries(Vec<LogEntry>);

impl LogEntries {

    /// We merge the new entries with the current ones. We assume that each index will always be correct and match
    /// the exact index of the log entry. We also assume that the new entries are always in order.
    fn merge(&mut self, new_entries: Vec<LogEntry>){
        for entry in new_entries {
            let idx = entry.index as usize;
            match self.get(idx){
                Some(log) => {
                    if log.term != entry.term {
                        self.0.truncate(idx);
                        self.0.push(entry);
                    }
                }
                None => {
                    self.0.push(entry);
                }
            }
        }

    }

    fn new_entry(&mut self, term: u64,command: String ){
        let idx = self.last().map(|e|e.index + 1).unwrap_or_default();
        self.0.push(LogEntry::new(idx,  term, command))
    }

    fn previous_log_entry_is_up_to_date(
        &self,
        prev_log_index: usize,
        prev_log_term: u64,
    ) -> bool {
        if prev_log_index + self.len() == 0 {
            return true;
        }
        match self.get(prev_log_index) {
            Some(log) => log.term.eq(&prev_log_term),

            None => {
                return false;
            }
        }
    }
}

impl Deref for LogEntries {
    type Target = Vec<LogEntry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

// impl DerefMut for LogEntries {
//     fn deref_mut(&mut self) -> &mut Self::Target {
//         &mut self.0
//     }
// }

struct State {
    heartbeat_latency: f64,
    last_heartbeat: Option<Instant>,
    current_term: u64,
    voted_for: Option<SocketAddr>,
    log_entries: LogEntries,
    commit_index: u64,
    last_applied: u64,
    role: Role,
    addr: SocketAddr,
    leader: Option<SocketAddr>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            heartbeat_latency: 0.0,
            last_heartbeat: None,
            current_term: 0,
            voted_for: None,
            log_entries: LogEntries::default(),
            commit_index: 0,
            last_applied: 0,
            role: Role::Follower,
            addr: SocketAddr::from_str("127.0.0.1:4000").unwrap(),
            leader: None
    }}
}

impl Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "role: {:?}", self.role)
    }
}

pub enum AppendEntriesError{
    OutOfDateTerm(u64),
    MissingLogEntries,
}


impl State {
    fn new(addr: SocketAddr, latency: f64)->Self{
        let mut state = Self::default();
        state.addr = addr;
        state.heartbeat_latency = latency;
        state
    }

    ///Invoked by leader to replicate log entries (§5.3); also used as heartbeat (§5.2).
    ///1. Reply false if term < currentTerm (§5.1)
/// 2. Reply false if log doesn’t contain an entry at prevLogIndex whose term matches prevLogTerm (§5.3)
/// 3. If an existing entry conflicts with a new one (same index but different terms), delete the existing entry and all that follow it (§5.3)
/// 4. Append any new entries not already in the log
/// 5. If leaderCommit > commitIndex, set commitIndex = min(leaderCommit, index of last new entry)
    fn handle_append_entries(
        &mut self,
        term: u64,
        leader_id: &str,
        prev_log_index: usize,
        prev_log_term: u64,
        leader_commit: u64,
        entries: Vec<LogEntry>,
    ) -> AppendEntriesResponse {
        //1
        if term < self.current_term {
            return AppendEntriesResponse::failed(self.current_term);
        }

        if self.role == Role::Candidate {
            self.role = Role::Follower;
            self.voted_for = None;
            self.leader = Some(SocketAddr::from_str(leader_id).expect("why leader_id isnt a correct socketaddrs?"));
        }

        let current_entries = &mut self.log_entries;

        //2
        if !current_entries.previous_log_entry_is_up_to_date(prev_log_index, prev_log_term){
            return AppendEntriesResponse::failed(term);
        }

        //3 and 4
       current_entries.merge(entries);

        //5
        if leader_commit > self.commit_index {
            self.commit_index = leader_commit;
        }

        return AppendEntriesResponse::successful(term);
    }



    fn handle_request_vote(
        &mut self,
        term: u64,
        candidate_id: &str,
        last_log_index: u64,
        last_log_term: u64,
    ) -> RequestVoteResponse {

        if term < self.current_term {
            return RequestVoteResponse::not_granted(self.current_term);
        }

        //NOTE: use something better for the id of the server
        let condidate_id_matches = self.voted_for.is_none_or(|addr| {
            addr.eq(&SocketAddr::from_str(candidate_id)
                .expect("why candidate_id isnt a correct socketaddrs?"))
        });

        let (last_term, last_index) = self.last_log_info();
        let logs_uptodate =
        last_log_term >= last_term && last_log_index >= last_index;

        match condidate_id_matches && logs_uptodate{
            true => {

                return RequestVoteResponse::granted(self.current_term);
            }
            false => {
                return RequestVoteResponse::not_granted(self.current_term);
            }
        };

    }

    fn last_log_info(&self) -> (u64, u64) {
        let last_log = self.log_entries.last();
        match last_log {
            Some(last_log) => (last_log.term, last_log.index),
            None => (0, 0),
        }
    }

    fn become_candidate(&mut self) {
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
        self.voted_for = None;
    }

    fn check_election(&mut self, election_timeout: Duration) {
        if self
            .last_heartbeat
            .is_none_or(|t| t.elapsed() >= election_timeout)
            && self.role == Role::Follower
        {
            self.become_candidate()
        }
    }
}

#[cfg(test)]
mod tests {
   use super::*;

    #[test]
    fn test_handle_append_entries_term_lower_than_current_follower() {
        let mut service = State::default();
        service.current_term = 1;

        let response = service.handle_append_entries(0, "127.0.0.1:4000", 0, 0, 0, vec![]);

        assert_eq!(response.success(), false);
        assert_eq!(response.term(), 1);
        assert_eq!(service.current_term, 1);
        assert_eq!(service.voted_for, None);
        assert_eq!(service.log_entries.len(), 0);
        assert_eq!(service.commit_index, 0);
        assert_eq!(service.last_applied, 0);
        assert_eq!(service.role, Role::Follower);
        assert_eq!(service.addr, SocketAddr::from_str("127.0.0.1:4000").unwrap());
        assert_eq!(service.leader, None);
    }

    #[test]
    fn test_handle_append_entries_term_lower_than_current_candidate() {
        let remote = Some(SocketAddr::from_str("127.0.0.1:4003").unwrap());
        let mut service = State::default();
        service.current_term = 1;
        service.role = Role::Candidate;
        service.voted_for = remote;
        service.leader = remote;

        let response = service.handle_append_entries(0, "127.0.0.1:4000", 0, 0, 0, vec![]);

        assert_eq!(response.success(), false);
        assert_eq!(response.term(), 1);
        assert_eq!(service.current_term, 1);
        assert_eq!(service.voted_for, remote);
        assert_eq!(service.log_entries.len(), 0);
        assert_eq!(service.commit_index, 0);
        assert_eq!(service.last_applied, 0);
        assert_eq!(service.role, Role::Candidate);
        assert_eq!(service.addr, SocketAddr::from_str("127.0.0.1:4000").unwrap());
        assert_eq!(service.leader, remote);
    }

    #[test]
    fn test_handle_append_entries_log_index_mismatch_follower() {
        let remote = Some(SocketAddr::from_str("127.0.0.1:4003").unwrap());
        let mut service = State::default();
        service.current_term = 1;
        service.voted_for = remote;
        service.leader = remote;
        service.log_entries = LogEntries(vec![
            LogEntry::new(0, 1, "command1".to_string()),
        ]);

        let entries = vec![
            LogEntry::new(2, 1, "command2".to_string()),
        ];

        let response = service.handle_append_entries(1, "127.0.0.1:4003", 1, 1, 1, entries);

        assert_eq!(response.success(), false);
        assert_eq!(response.term(), 1);
        assert_eq!(service.current_term, 1);
        assert_eq!(service.voted_for, remote);
        assert_eq!(service.log_entries.len(), 1);
        assert_eq!(service.log_entries[0].index, 0);
        assert_eq!(service.log_entries[0].term, 1);
        assert_eq!(service.log_entries[0].command, "command1");
        assert_eq!(service.commit_index, 0);
        assert_eq!(service.last_applied, 0);
        assert_eq!(service.role, Role::Follower);
        assert_eq!(service.addr, SocketAddr::from_str("127.0.0.1:4000").unwrap());
        assert_eq!(service.leader, remote);
    }

    #[test]
    fn test_handle_append_entries_log_index_mismatch_candidate() {
        let remote = Some(SocketAddr::from_str("127.0.0.1:4004").unwrap());
        let mut service = State::default();
        service.current_term = 1;
        service.voted_for = remote;
        service.role = Role::Candidate;
        service.leader = remote;
        service.log_entries = LogEntries(vec![
            LogEntry::new(0, 1, "command1".to_string()),
        ]);

        let entries = vec![
            LogEntry::new(2, 1, "command2".to_string()),
        ];

        let response = service.handle_append_entries(1, "127.0.0.1:4003", 1, 1, 1, entries);

        assert_eq!(response.success(), false);
        assert_eq!(response.term(), 1);
        assert_eq!(service.current_term, 1);
        assert_eq!(service.voted_for, None);
        assert_eq!(service.log_entries.len(), 1);
        assert_eq!(service.log_entries[0].index, 0);
        assert_eq!(service.log_entries[0].term, 1);
        assert_eq!(service.log_entries[0].command, "command1");
        assert_eq!(service.commit_index, 0);
        assert_eq!(service.last_applied, 0);
        assert_eq!(service.role, Role::Follower);
        assert_eq!(service.addr, SocketAddr::from_str("127.0.0.1:4000").unwrap());
        assert_eq!(service.leader, Some(SocketAddr::from_str("127.0.0.1:4003").unwrap()));
    }

    #[test]
    fn test_handle_append_entries_log_term_mismatch_candidate() {
        let remote = Some(SocketAddr::from_str("127.0.0.1:4004").unwrap());
        let mut service = State::default();
        service.current_term = 1;
        service.voted_for = remote;
        service.role = Role::Candidate;
        service.leader = remote;
        service.log_entries = LogEntries(vec![
            LogEntry::new(0, 1, "command1".to_string()),
        ]);

        let entries = vec![
            LogEntry::new(1, 2, "command2".to_string()),
        ];

        let response = service.handle_append_entries(2, "127.0.0.1:4003", 0, 2, 1, entries);

        assert_eq!(response.success(), false);
        assert_eq!(response.term(), 2);
        assert_eq!(service.current_term, 1);
        assert_eq!(service.voted_for, None);
        assert_eq!(service.log_entries.len(), 1);
        assert_eq!(service.log_entries[0].index, 0);
        assert_eq!(service.log_entries[0].term, 1);
        assert_eq!(service.log_entries[0].command, "command1");
        assert_eq!(service.commit_index, 0);
        assert_eq!(service.last_applied, 0);
        assert_eq!(service.role, Role::Follower);
        assert_eq!(service.addr, SocketAddr::from_str("127.0.0.1:4000").unwrap());
        assert_eq!(service.leader, Some(SocketAddr::from_str("127.0.0.1:4003").unwrap()));
    }

    #[test]
    fn test_handle_append_entries_update_commit_index() {
        let remote = Some(SocketAddr::from_str("127.0.0.1:4004").unwrap());
        let mut service = State::default();
        service.current_term = 1;
        service.voted_for = remote;
        service.role = Role::Candidate;
        service.leader = remote;
        service.log_entries = LogEntries(vec![
            LogEntry::new(0, 1, "command1".to_string()),
        ]);

        let entries = vec![
            LogEntry::new(1, 2, "command2".to_string()),
        ];

        let response = service.handle_append_entries(2, "127.0.0.1:4003", 0,1, 1, entries);

        assert_eq!(response.success(), true);
        assert_eq!(response.term(), 2);
        assert_eq!(service.current_term, 1);
        assert_eq!(service.voted_for, None);
        assert_eq!(service.log_entries.len(), 2);
        assert_eq!(service.log_entries[0].index, 0);
        assert_eq!(service.log_entries[0].term, 1);
        assert_eq!(service.log_entries[0].command, "command1");
        assert_eq!(service.log_entries[1].index, 1);
        assert_eq!(service.log_entries[1].term, 2);
        assert_eq!(service.log_entries[1].command, "command2");
        assert_eq!(service.commit_index, 1);
        assert_eq!(service.last_applied, 0);
        assert_eq!(service.role, Role::Follower);
        assert_eq!(service.addr, SocketAddr::from_str("127.0.0.1:4000").unwrap());
        assert_eq!(service.leader, Some(SocketAddr::from_str("127.0.0.1:4003").unwrap()));
    }


    #[test]
    fn test_new_log_entry() {
        let mut entries = LogEntries::default();
        entries.new_entry(1, "command1".to_string());

        assert_eq!(entries[0].index, 0);
        assert_eq!(entries[0].term, 1);
        assert_eq!(entries[0].command, "command1");
    }

    #[test]
    fn test_new_log_entry_new_index() {
        let mut entries = LogEntries::default();
        entries.new_entry(1, "command1".to_string());
        entries.new_entry(1, "command2".to_string());

        assert_eq!(entries[0].index, 0);
        assert_eq!(entries[0].term, 1);
        assert_eq!(entries[0].command, "command1");

        assert_eq!(entries[1].index, 1);
        assert_eq!(entries[1].term, 1);
        assert_eq!(entries[1].command, "command2");
    }


    #[test]
    fn test_previous_log_entry_is_up_to_date_correct() {
        let entries = LogEntries(vec![
            LogEntry::new(0, 1, "command1".to_string()),
            LogEntry::new(1, 1, "command2".to_string()),
        ]);

        let is_up_to_date = entries.previous_log_entry_is_up_to_date(1, 1);
        assert!(is_up_to_date);
    }

    #[test]
    fn test_previous_log_entry_is_up_to_date_no_index() {
        let entries = LogEntries::default();

        let is_up_to_date = entries.previous_log_entry_is_up_to_date(1, 1);
        assert!(!is_up_to_date);
    }

    #[test]
    fn test_previous_log_entry_is_up_to_date_no_term_matching() {
        let entries = LogEntries(vec![
            LogEntry::new(0, 1, "command1".to_string()),
            LogEntry::new(1, 1, "command2".to_string()),
        ]);

        let is_up_to_date = entries.previous_log_entry_is_up_to_date(1, 2);
        assert!(!is_up_to_date);
    }

    #[test]
    fn test_previous_log_entry_is_up_to_date_test_0_empty() {
        let entries = LogEntries::default();

        let is_up_to_date = entries.previous_log_entry_is_up_to_date(0, 1);
        assert!(is_up_to_date);
    }

    #[test]
    fn test_previous_log_entry_is_up_to_date_test_0() {
        let entries = LogEntries(vec![
            LogEntry::new(0, 1, "command1".to_string()),
        ]);

        let is_up_to_date = entries.previous_log_entry_is_up_to_date(0, 1);
        assert!(is_up_to_date);
    }

    #[test]
    fn test_previous_log_entry_is_up_to_date_empty_entries() {
        let entries = LogEntries::default();

        let is_up_to_date = entries.previous_log_entry_is_up_to_date(0, 1);
        assert!(is_up_to_date);
    }

    #[test]
    fn test_merge_add_new() {
        let mut entries = LogEntries(vec![
            LogEntry::new(0, 1, "command1".to_string()),
            LogEntry::new(1, 1, "command2".to_string()),
        ]);

        let new_entries = vec![
            LogEntry::new(2, 2, "command3".to_string()),
        ];

         entries.merge(new_entries);

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].index, 0);
        assert_eq!(entries[0].term, 1);
        assert_eq!(entries[0].command, "command1");
        assert_eq!(entries[1].index, 1);
        assert_eq!(entries[1].term, 1);
        assert_eq!(entries[1].command, "command2");
        assert_eq!(entries[2].index, 2);
        assert_eq!(entries[2].term, 2);
        assert_eq!(entries[2].command, "command3");
    }

    #[test]
    fn test_merge_ignore_entry() {
        let mut entries = LogEntries(vec![
            LogEntry::new(0, 1, "command1".to_string()),
            LogEntry::new(1, 1, "command2".to_string()),
        ]);

        let new_entries = vec![
            LogEntry::new(1, 1, "command2".to_string()),
        ];

         entries.merge(new_entries);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, 0);
        assert_eq!(entries[0].term, 1);
        assert_eq!(entries[0].command, "command1");
        assert_eq!(entries[1].index, 1);
        assert_eq!(entries[1].term, 1);
        assert_eq!(entries[1].command, "command2");
    }

    #[test]
    fn test_merge_change_incorrect_entries() {
        let mut entries = LogEntries(vec![
            LogEntry::new(0, 1, "command1".to_string()),
            LogEntry::new(1, 1, "command2".to_string()),
            LogEntry::new(2, 1, "command3".to_string()),
        ]);

        let new_entries = vec![
            LogEntry::new(0, 2, "newcommand1".to_string()),
            LogEntry::new(1, 2, "newcommand2".to_string()),
        ];

         entries.merge(new_entries);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, 0);
        assert_eq!(entries[0].term, 2);
        assert_eq!(entries[0].command, "newcommand1");
        assert_eq!(entries[1].index, 1);
        assert_eq!(entries[1].term, 2);
        assert_eq!(entries[1].command, "newcommand2");
    }

}
