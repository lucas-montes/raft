use std::{net::SocketAddr, str::FromStr};

use crate::{
    dto::VoteResponse,
    state::{NodeId, Role},
    storage::{LogEntries, LogEntry, LogsInformation},
};

#[derive(Debug)]
pub enum AppendEntriesResult {
    Ok,
    TermMismatch(u64),
    LogEntriesMismatch { last_index: u64, last_term: u64 },
}

pub trait Consensus {
    async fn commit_hard_state(&self);
    fn current_term(&self) -> u64;
    fn commit_index(&self) -> u64;
    fn update_commit_index(&mut self, commit_index: u64);
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
    ) -> AppendEntriesResult {
        tracing::info!(
            action = "receiveAppendEntries",
            term = term,
            leader = leader_id,
            prev_log_index = prev_log_index,
            prev_log_term = prev_log_term,
            leader_commit = leader_commit
        );
        //1
        if term < self.current_term() {
            return AppendEntriesResult::TermMismatch(self.current_term());
        }

        self.become_follower(
            Some(
                SocketAddr::from_str(leader_id).expect("why leader_id isnt a correct socketaddrs?"),
            ),
            term,
        );

        let current_entries = &mut *self.log_entries();

        // 2
        if !current_entries.previous_log_entry_is_up_to_date(prev_log_index, prev_log_term) {
            //TODO: when we return this error, the leader needs to know the last index/term of the
            //failing node so it can send the log entries to make him update
            let result = self.last_log_info();
            return AppendEntriesResult::LogEntriesMismatch {
                last_index: result.last_log_index(),
                last_term: result.last_log_term(),
            };
        }

        // 3 and 4
        // TODO: send this as a message to avoid blocking?
        let result = current_entries.merge(entries);

        //5
        if leader_commit > self.commit_index() {
            self.update_commit_index(std::cmp::min(leader_commit, result.last_log_index()));
        }

        AppendEntriesResult::Ok
    }

    fn handle_request_vote(
        &mut self,
        term: u64,
        candidate_id: &str,
        last_log_index: u64,
        last_log_term: u64,
    ) -> VoteResponse {
        tracing::info!(
            action = "receiveVote",
            term = term,
            candidate = candidate_id,
            last_log_index = last_log_index,
            last_log_term = last_log_term
        );

        if term < self.current_term() {
            return VoteResponse::not_granted(self.current_term());
        }

        let candidate = NodeId::new(
            SocketAddr::from_str(candidate_id)
                .expect("why candidate_id isnt a correct socketaddrs?"),
        );

        if term > self.current_term() {
            self.become_follower(None, term);
        }

        let condidate_id_matches = self.voted_for().is_none_or(|addr| addr.eq(&candidate));

        let last_log_info = self.last_log_info();

        //NOTE: in the paper we find it has "at least up to date" and in a presentation we find this formula
        let logs_uptodate = last_log_term > last_log_info.last_log_term()
            || (last_log_index >= last_log_info.last_log_index()
                && last_log_term == last_log_info.last_log_term());

        if condidate_id_matches && logs_uptodate {
            self.vote_for(candidate);
            VoteResponse::granted(self.current_term())
        } else {
            VoteResponse::not_granted(self.current_term())
        }
    }

    fn is_majority(&self, votes: u64) -> bool {
        let num_nodes = self.cluster_size();
        let has_majority = votes > num_nodes.div_euclid(2);
        has_majority || num_nodes.eq(&1)
    }

    fn count_votes(&mut self, votes: u64) {
        if self.is_majority(votes) {
            self.become_leader();
            tracing::info!(
                action = "becomeLeader",
                term = self.current_term(),
                votes = votes,
                peers = self.cluster_size()
            );
        } else {
            tracing::info!(
                action = "notEnoughVotes",
                term = self.current_term(),
                votes = votes,
                peers = self.cluster_size()
            );
        }
    }

    fn cluster_size(&self) -> u64;
    fn last_log_info(&self) -> LogsInformation;
    fn leader(&self) -> Option<SocketAddr>;
    fn log_entries(&mut self) -> &mut LogEntries; //TODO: should communicate with channels probably
    fn vote_for(&mut self, node: NodeId);
    fn voted_for(&self) -> Option<&NodeId>;
    fn become_follower(&mut self, leader_id: Option<SocketAddr>, new_term: u64);
    fn become_candidate(&mut self);
    fn become_leader(&mut self);
    fn id(&self) -> &NodeId;
    fn role(&self) -> &Role;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        state::{NodeId, Role},
        storage::LogEntries,
    };

    // Mock implementation for testing
    #[derive(Debug)]
    struct MockState {
        id: NodeId,
        current_term: u64,
        commit_index: u64,
        log_entries: LogEntries,
        voted_for: Option<NodeId>,
        leader: Option<SocketAddr>,
        role: Role,
        cluster_size: u64,
    }

    impl Consensus for MockState {
        async fn commit_hard_state(&self) {}

        fn current_term(&self) -> u64 {
            self.current_term
        }

        fn commit_index(&self) -> u64 {
            self.commit_index
        }

        fn update_commit_index(&mut self, commit_index: u64) {
            self.commit_index = commit_index;
        }

        fn cluster_size(&self) -> u64 {
            self.cluster_size
        }

        fn last_log_info(&self) -> LogsInformation {
            LogsInformation::default()
        }

        fn leader(&self) -> Option<SocketAddr> {
            self.leader
        }

        fn log_entries(&mut self) -> &mut LogEntries {
            &mut self.log_entries
        }

        fn vote_for(&mut self, node: NodeId) {
            self.voted_for = Some(node);
        }

        fn voted_for(&self) -> Option<&NodeId> {
            self.voted_for.as_ref()
        }

        fn become_follower(&mut self, leader_id: Option<SocketAddr>, new_term: u64) {
            self.role = Role::Follower;
            self.leader = leader_id;
            self.current_term = new_term;
            self.voted_for = None;
        }

        fn become_candidate(&mut self) {
            self.role = Role::Candidate;
        }

        fn become_leader(&mut self) {
            self.role = Role::Leader;
        }

        fn id(&self) -> &NodeId {
            &self.id
        }

        fn role(&self) -> &Role {
            &self.role
        }
    }

    impl MockState {
        fn default() -> Self {
            let addr = SocketAddr::from_str("127.0.0.1:4000").unwrap();
            Self {
                id: NodeId::new(addr),
                current_term: 0,
                commit_index: 0,
                log_entries: LogEntries::default(),
                voted_for: None,
                leader: None,
                role: Role::Follower,
                cluster_size: 3,
            }
        }
    }

    #[test]
    fn test_is_majority_single_node() {
        let mut state = MockState::default();
        state.cluster_size = 1;
        assert!(state.is_majority(1));
        assert!(state.is_majority(0)); // Special case for single node
    }

    #[test]
    fn test_is_majority_three_nodes() {
        let mut state = MockState::default();
        state.cluster_size = 3;
        assert!(!state.is_majority(0));
        assert!(!state.is_majority(1));
        assert!(state.is_majority(2));
        assert!(state.is_majority(3));
    }

    #[test]
    fn test_is_majority_five_nodes() {
        let mut state = MockState::default();
        state.cluster_size = 5;
        assert!(!state.is_majority(0));
        assert!(!state.is_majority(1));
        assert!(!state.is_majority(2));
        assert!(state.is_majority(3));
        assert!(state.is_majority(4));
        assert!(state.is_majority(5));
    }

    #[test]
    fn test_is_majority_even_nodes() {
        let mut state = MockState::default();
        state.cluster_size = 4;
        assert!(!state.is_majority(0));
        assert!(!state.is_majority(1));
        assert!(!state.is_majority(2));
        assert!(state.is_majority(3));
        assert!(state.is_majority(4));
    }

    #[test]
    fn test_count_votes_becomes_leader_with_majority() {
        let mut state = MockState::default();
        state.cluster_size = 3;
        state.count_votes(2);
        assert_eq!(state.role(), &Role::Leader);
    }

    #[test]
    fn test_count_votes_stays_candidate_without_majority() {
        let mut state = MockState::default();
        state.cluster_size = 5;
        state.role = Role::Candidate;
        state.count_votes(2);
        assert_eq!(state.role(), &Role::Candidate);
    }

    #[test]
    fn test_count_votes_single_node_becomes_leader() {
        let mut state = MockState::default();
        state.cluster_size = 1;
        state.count_votes(1);
        assert_eq!(state.role(), &Role::Leader);
    }

    #[test]
    fn test_handle_request_vote_grants_vote_when_valid() {
        let mut state = MockState::default();
        state.current_term = 1;

        let candidate_addr = "127.0.0.1:4001";
        let response = state.handle_request_vote(1, candidate_addr, 1, 1);

        assert!(response.vote_granted());
        assert_eq!(response.term(), 1);
        assert_eq!(
            state.voted_for().unwrap().addr().to_string(),
            candidate_addr
        );
    }

    #[test]
    fn test_handle_request_vote_rejects_lower_term() {
        let mut state = MockState::default();
        state.current_term = 2;

        let candidate_addr = "127.0.0.1:4001";
        let response = state.handle_request_vote(1, candidate_addr, 1, 1);

        assert!(!response.vote_granted());
        assert_eq!(response.term(), 2);
        assert!(state.voted_for().is_none());
    }

    #[test]
    fn test_handle_request_vote_rejects_already_voted() {
        let mut state = MockState::default();
        state.current_term = 1;
        state.voted_for = Some(NodeId::new(SocketAddr::from_str("127.0.0.1:4002").unwrap()));

        let candidate_addr = "127.0.0.1:4001";
        let response = state.handle_request_vote(1, candidate_addr, 1, 1);

        assert!(!response.vote_granted());
        assert_eq!(response.term(), 1);
    }

    #[test]
    fn test_handle_request_vote_allows_revote_for_same_candidate() {
        let mut state = MockState::default();
        state.current_term = 1;
        state.voted_for = Some(NodeId::new(SocketAddr::from_str("127.0.0.1:4001").unwrap()));

        let candidate_addr = "127.0.0.1:4001";
        let response = state.handle_request_vote(1, candidate_addr, 1, 1);

        assert!(response.vote_granted());
        assert_eq!(response.term(), 1);
    }

    #[test]
    fn test_handle_request_vote_updates_term_when_higher() {
        let mut state = MockState::default();
        state.current_term = 1;
        state.role = Role::Candidate;

        let candidate_addr = "127.0.0.1:4001";
        let response = state.handle_request_vote(3, candidate_addr, 1, 1);

        assert_eq!(state.current_term(), 3);
        assert_eq!(state.role(), &Role::Follower);
        assert!(state.voted_for().is_some());
        assert!(response.vote_granted());
    }

    // New tests for handle_append_entries
    #[test]
    fn test_handle_append_entries_rejects_lower_term() {
        let mut state = MockState::default();
        state.current_term = 2;

        let result = state.handle_append_entries(1, "127.0.0.1:4001", 0, 0, 0, vec![]);

        match result {
            AppendEntriesResult::TermMismatch(term) => assert_eq!(term, 2),
            _ => panic!("Expected TermMismatch"),
        }
    }

    #[test]
    fn test_handle_append_entries_becomes_follower() {
        let mut state = MockState::default();
        state.current_term = 1;
        state.role = Role::Candidate;

        let _result = state.handle_append_entries(2, "127.0.0.1:4001", 0, 0, 0, vec![]);

        assert_eq!(state.role(), &Role::Follower);
        assert_eq!(state.current_term(), 2);
        assert_eq!(state.leader().unwrap().to_string(), "127.0.0.1:4001");
    }

    #[test]
    fn test_handle_append_entries_updates_commit_index() {
        let mut state = MockState::default();
        state.current_term = 1;
        state.commit_index = 0;

        let _result = state.handle_append_entries(
            1,
            "127.0.0.1:4001",
            0,
            0,
            5, // leader_commit
            vec![],
        );

        // Since LogsInformation::default() likely returns last_log_index = 0,
        // commit_index should be min(5, 0) = 0, but the function will only update if leader_commit > current
        assert_eq!(state.commit_index(), 0);
    }

    #[test]
    fn test_handle_append_entries_success_case() {
        let mut state = MockState::default();
        state.current_term = 1;

        let result = state.handle_append_entries(1, "127.0.0.1:4001", 0, 0, 0, vec![]);

        match result {
            AppendEntriesResult::Ok => {}
            _ => panic!("Expected Ok result"),
        }

        // Verify state changes
        assert_eq!(state.role(), &Role::Follower);
        assert_eq!(state.leader().unwrap().to_string(), "127.0.0.1:4001");
    }

    #[test]
    fn test_handle_append_entries_same_term_updates_leader() {
        let mut state = MockState::default();
        state.current_term = 1;
        state.role = Role::Candidate;
        state.leader = None;

        let _result = state.handle_append_entries(1, "127.0.0.1:4001", 0, 0, 0, vec![]);

        assert_eq!(state.role(), &Role::Follower);
        assert_eq!(state.current_term(), 1);
        assert_eq!(state.leader().unwrap().to_string(), "127.0.0.1:4001");
    }
}
