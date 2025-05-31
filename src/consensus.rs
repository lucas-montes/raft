use std::{net::SocketAddr, str::FromStr};

use crate::{
    dto::VoteResponse, peers::Peers, state::NodeId, storage::{LogEntries, LogEntry, LogsInformation}
};

#[derive(Debug)]
pub enum AppendEntriesResult {
    Ok,
    TermMismatch(u64),
    LogEntriesMismatch { last_index: u64, last_term: u64 },
}

pub trait Consensus {
    async fn commit_hard_state(&self);
    async fn restart_peer(&mut self, peer_index: usize); //TODO: doesnt belong here
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

        // //2
        if !current_entries.previous_log_entry_is_up_to_date(prev_log_index, prev_log_term) {
            //TODO: when we return this error, the leader needs to know the last index/term of the
            //failing node so it can send the log entries to make him update
            let result = self.last_log_info();
            return AppendEntriesResult::LogEntriesMismatch {
                last_index: result.last_log_index(),
                last_term: result.last_log_term(),
            };
        }

        // //3 and 4
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

    fn count_votes(&mut self, votes: u64) {
        let num_nodes = self.peers().total_connected();
        let has_majority = votes > num_nodes.div_euclid(2) as u64;
        if has_majority || num_nodes.eq(&1) {
            self.become_leader();
            tracing::info!(
                action = "becomeLeader",
                term = self.current_term(),
                votes = votes,
                peers = num_nodes
            );
        } else {
            tracing::info!(
                action = "notEnoughVotes",
                term = self.current_term(),
                votes = votes,
                peers = num_nodes
            );
        }
    }

    fn last_log_info(&self) -> LogsInformation;
    fn peers(&self) -> &Peers;
    fn leader(&self) -> Option<SocketAddr>;
    fn log_entries(&mut self) -> &mut LogEntries; //TODO: should communicate with channels probably
    fn vote_for(&mut self, node: NodeId);
    fn voted_for(&self) -> Option<&NodeId>;
    fn become_follower(&mut self, leader_id: Option<SocketAddr>, new_term: u64);
    fn become_candidate(&mut self);
    fn become_leader(&mut self);
    fn id(&self) -> &NodeId;
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_handle_append_entries_term_lower_than_current_follower() {
//         let mut service = State::default();
//         service.current_term = 1;

//         let response = service.handle_append_entries(0, "127.0.0.1:4000", 0, 0, 0, vec![]);

//         match response.unwrap_err() {
//             AppendEntriesError::TermMismatch(term) => {
//                 assert_eq!(term, 1);
//             }
//             _ => panic!("Expected TermMismatch error"),
//         }

//         assert_eq!(service.current_term, 1);
//         assert_eq!(service.voted_for, None);
//         assert_eq!(service.log_entries.len(), 0);
//         assert_eq!(service.commit_index, 0);
//         assert_eq!(service.last_applied, 0);
//         assert_eq!(service.role, Role::Follower);
//         assert_eq!(
//             service.addr,
//             SocketAddr::from_str("127.0.0.1:4000").unwrap()
//         );
//         assert_eq!(service.leader, None);
//     }

//     #[test]
//     fn test_handle_append_entries_term_lower_than_current_candidate() {
//         let remote = Some(SocketAddr::from_str("127.0.0.1:4003").unwrap());
//         let mut service = State::default();
//         service.current_term = 1;
//         service.role = Role::Candidate;
//         service.voted_for = remote;
//         service.leader = remote;

//         let response = service.handle_append_entries(0, "127.0.0.1:4000", 0, 0, 0, vec![]);

//         match response.unwrap_err() {
//             AppendEntriesError::TermMismatch(term) => {
//                 assert_eq!(term, 1);
//             }
//             _ => panic!("Expected TermMismatch error"),
//         }
//         assert_eq!(service.current_term, 1);
//         assert_eq!(service.voted_for, remote);
//         assert_eq!(service.log_entries.len(), 0);
//         assert_eq!(service.commit_index, 0);
//         assert_eq!(service.last_applied, 0);
//         assert_eq!(service.role, Role::Candidate);
//         assert_eq!(
//             service.addr,
//             SocketAddr::from_str("127.0.0.1:4000").unwrap()
//         );
//         assert_eq!(service.leader, remote);
//     }

//     #[test]
//     fn test_handle_append_entries_log_index_mismatch_follower() {
//         let remote = Some(SocketAddr::from_str("127.0.0.1:4003").unwrap());
//         let mut service = State::default();
//         service.current_term = 1;
//         service.voted_for = remote;
//         service.leader = remote;
//         service.log_entries = LogEntries(vec![LogEntry::new(0, 1, "command1".to_string())]);

//         let entries = vec![LogEntry::new(2, 1, "command2".to_string())];

//         let response = service.handle_append_entries(1, "127.0.0.1:4003", 1, 1, 1, entries);

//         match response.unwrap_err() {
//             AppendEntriesError::LogEntriesMismatch {
//                 last_index,
//                 last_term,
//             } => {
//                 assert_eq!(last_index, 0);
//                 assert_eq!(last_term, 1);
//             }
//             _ => panic!("Expected LogEntriesMismatch error"),
//         }

//         assert_eq!(service.current_term, 1);
//         assert_eq!(service.voted_for, None);
//         assert_eq!(service.log_entries.len(), 1);
//         assert_eq!(service.log_entries[0].index, 0);
//         assert_eq!(service.log_entries[0].term, 1);
//         assert_eq!(service.log_entries[0].command, "command1");
//         assert_eq!(service.commit_index, 0);
//         assert_eq!(service.last_applied, 0);
//         assert_eq!(service.role, Role::Follower);
//         assert_eq!(
//             service.addr,
//             SocketAddr::from_str("127.0.0.1:4000").unwrap()
//         );
//         assert_eq!(service.leader, remote);
//     }

//     #[test]
//     fn test_handle_append_entries_log_index_mismatch_candidate() {
//         let remote = Some(SocketAddr::from_str("127.0.0.1:4004").unwrap());
//         let mut service = State::default();
//         service.current_term = 1;
//         service.voted_for = remote;
//         service.role = Role::Candidate;
//         service.leader = remote;
//         service.log_entries = LogEntries(vec![LogEntry::new(0, 1, "command1".to_string())]);

//         let entries = vec![LogEntry::new(2, 1, "command2".to_string())];

//         let response = service.handle_append_entries(1, "127.0.0.1:4003", 1, 1, 1, entries);

//         match response.unwrap_err() {
//             AppendEntriesError::LogEntriesMismatch {
//                 last_index,
//                 last_term,
//             } => {
//                 assert_eq!(last_index, 0);
//                 assert_eq!(last_term, 1);
//             }
//             _ => panic!("Expected LogEntriesMismatch error"),
//         }
//         assert_eq!(service.current_term, 1);
//         assert_eq!(service.voted_for, None);
//         assert_eq!(service.log_entries.len(), 1);
//         assert_eq!(service.log_entries[0].index, 0);
//         assert_eq!(service.log_entries[0].term, 1);
//         assert_eq!(service.log_entries[0].command, "command1");
//         assert_eq!(service.commit_index, 0);
//         assert_eq!(service.last_applied, 0);
//         assert_eq!(service.role, Role::Follower);
//         assert_eq!(
//             service.addr,
//             SocketAddr::from_str("127.0.0.1:4000").unwrap()
//         );
//         assert_eq!(
//             service.leader,
//             Some(SocketAddr::from_str("127.0.0.1:4003").unwrap())
//         );
//     }

//     #[test]
//     fn test_handle_append_entries_log_term_mismatch_candidate() {
//         let remote = Some(SocketAddr::from_str("127.0.0.1:4004").unwrap());
//         let mut service = State::default();
//         service.current_term = 1;
//         service.voted_for = remote;
//         service.role = Role::Candidate;
//         service.leader = remote;
//         service.log_entries = LogEntries(vec![LogEntry::new(0, 1, "command1".to_string())]);

//         let entries = vec![LogEntry::new(1, 2, "command2".to_string())];

//         let response = service.handle_append_entries(2, "127.0.0.1:4003", 0, 2, 1, entries);

//         match response.unwrap_err() {
//             AppendEntriesError::LogEntriesMismatch {
//                 last_index,
//                 last_term,
//             } => {
//                 assert_eq!(last_index, 0);
//                 assert_eq!(last_term, 1);
//             }
//             _ => panic!("Expected LogEntriesMismatch error"),
//         }

//         assert_eq!(service.current_term, 2);
//         assert_eq!(service.voted_for, None);
//         assert_eq!(service.log_entries.len(), 1);
//         assert_eq!(service.log_entries[0].index, 0);
//         assert_eq!(service.log_entries[0].term, 1);
//         assert_eq!(service.log_entries[0].command, "command1");
//         assert_eq!(service.commit_index, 0);
//         assert_eq!(service.last_applied, 0);
//         assert_eq!(service.role, Role::Follower);
//         assert_eq!(
//             service.addr,
//             SocketAddr::from_str("127.0.0.1:4000").unwrap()
//         );
//         assert_eq!(
//             service.leader,
//             Some(SocketAddr::from_str("127.0.0.1:4003").unwrap())
//         );
//     }

//     #[test]
//     fn test_handle_append_entries_update_commit_index() {
//         let remote = Some(SocketAddr::from_str("127.0.0.1:4004").unwrap());
//         let mut service = State::default();
//         service.current_term = 1;
//         service.voted_for = remote;
//         service.role = Role::Candidate;
//         service.leader = remote;
//         service.log_entries = LogEntries(vec![LogEntry::new(0, 1, "command1".to_string())]);

//         let entries = vec![LogEntry::new(1, 2, "command2".to_string())];

//         let response = service.handle_append_entries(2, "127.0.0.1:4003", 0, 1, 1, entries);

//         assert!(response.is_ok());
//         assert_eq!(service.current_term, 2);
//         assert_eq!(service.voted_for, None);
//         assert_eq!(service.log_entries.len(), 2);
//         assert_eq!(service.log_entries[0].index, 0);
//         assert_eq!(service.log_entries[0].term, 1);
//         assert_eq!(service.log_entries[0].command, "command1");
//         assert_eq!(service.log_entries[1].index, 1);
//         assert_eq!(service.log_entries[1].term, 2);
//         assert_eq!(service.log_entries[1].command, "command2");
//         assert_eq!(service.commit_index, 1);
//         assert_eq!(service.last_applied, 0);
//         assert_eq!(service.role, Role::Follower);
//         assert_eq!(
//             service.addr,
//             SocketAddr::from_str("127.0.0.1:4000").unwrap()
//         );
//         assert_eq!(
//             service.leader,
//             Some(SocketAddr::from_str("127.0.0.1:4003").unwrap())
//         );
//     }

//     // #[test]
//     // fn test_new_log_entry() {
//     //     let mut entries = LogEntries::default();
//     //     entries.new_entry(1, "command1".to_string());

//     //     assert_eq!(entries[0].index, 0);
//     //     assert_eq!(entries[0].term, 1);
//     //     assert_eq!(entries[0].command, "command1");
//     // }

//     // #[test]
//     // fn test_new_log_entry_new_index() {
//     //     let mut entries = LogEntries::default();
//     //     entries.new_entry(1, "command1".to_string());
//     //     entries.new_entry(1, "command2".to_string());

//     //     assert_eq!(entries[0].index, 0);
//     //     assert_eq!(entries[0].term, 1);
//     //     assert_eq!(entries[0].command, "command1");

//     //     assert_eq!(entries[1].index, 1);
//     //     assert_eq!(entries[1].term, 1);
//     //     assert_eq!(entries[1].command, "command2");
//     // }

//     #[test]
//     fn test_previous_log_entry_is_up_to_date_correct() {
//         let entries = LogEntries(vec![
//             LogEntry::new(0, 1, "command1".to_string()),
//             LogEntry::new(1, 1, "command2".to_string()),
//         ]);

//         let is_up_to_date = entries.previous_log_entry_is_up_to_date(1, 1);
//         assert!(is_up_to_date);
//     }

//     #[test]
//     fn test_previous_log_entry_is_up_to_date_no_index() {
//         let entries = LogEntries::default();

//         let is_up_to_date = entries.previous_log_entry_is_up_to_date(1, 1);
//         assert!(!is_up_to_date);
//     }

//     #[test]
//     fn test_previous_log_entry_is_up_to_date_no_term_matching() {
//         let entries = LogEntries(vec![
//             LogEntry::new(0, 1, "command1".to_string()),
//             LogEntry::new(1, 1, "command2".to_string()),
//         ]);

//         let is_up_to_date = entries.previous_log_entry_is_up_to_date(1, 2);
//         assert!(!is_up_to_date);
//     }

//     #[test]
//     fn test_previous_log_entry_is_up_to_date_test_0_empty() {
//         let entries = LogEntries::default();

//         let is_up_to_date = entries.previous_log_entry_is_up_to_date(0, 1);
//         assert!(is_up_to_date);
//     }

//     #[test]
//     fn test_previous_log_entry_is_up_to_date_test_0() {
//         let entries = LogEntries(vec![LogEntry::new(0, 1, "command1".to_string())]);

//         let is_up_to_date = entries.previous_log_entry_is_up_to_date(0, 1);
//         assert!(is_up_to_date);
//     }

//     #[test]
//     fn test_previous_log_entry_is_up_to_date_empty_entries() {
//         let entries = LogEntries::default();

//         let is_up_to_date = entries.previous_log_entry_is_up_to_date(0, 1);
//         assert!(is_up_to_date);
//     }

//     #[test]
//     fn test_merge_add_new() {
//         let mut entries = LogEntries(vec![
//             LogEntry::new(0, 1, "command1".to_string()),
//             LogEntry::new(1, 1, "command2".to_string()),
//         ]);

//         let new_entries = vec![LogEntry::new(2, 2, "command3".to_string())];

//         entries.merge(new_entries);

//         assert_eq!(entries.len(), 3);
//         assert_eq!(entries[0].index, 0);
//         assert_eq!(entries[0].term, 1);
//         assert_eq!(entries[0].command, "command1");
//         assert_eq!(entries[1].index, 1);
//         assert_eq!(entries[1].term, 1);
//         assert_eq!(entries[1].command, "command2");
//         assert_eq!(entries[2].index, 2);
//         assert_eq!(entries[2].term, 2);
//         assert_eq!(entries[2].command, "command3");
//     }

//     #[test]
//     fn test_merge_ignore_entry() {
//         let mut entries = LogEntries(vec![
//             LogEntry::new(0, 1, "command1".to_string()),
//             LogEntry::new(1, 1, "command2".to_string()),
//         ]);

//         let new_entries = vec![LogEntry::new(1, 1, "command2".to_string())];

//         entries.merge(new_entries);

//         assert_eq!(entries.len(), 2);
//         assert_eq!(entries[0].index, 0);
//         assert_eq!(entries[0].term, 1);
//         assert_eq!(entries[0].command, "command1");
//         assert_eq!(entries[1].index, 1);
//         assert_eq!(entries[1].term, 1);
//         assert_eq!(entries[1].command, "command2");
//     }

//     #[test]
//     fn test_merge_change_incorrect_entries() {
//         let mut entries = LogEntries(vec![
//             LogEntry::new(0, 1, "command1".to_string()),
//             LogEntry::new(1, 1, "command2".to_string()),
//             LogEntry::new(2, 1, "command3".to_string()),
//         ]);

//         let new_entries = vec![
//             LogEntry::new(0, 2, "newcommand1".to_string()),
//             LogEntry::new(1, 2, "newcommand2".to_string()),
//         ];

//         entries.merge(new_entries);

//         assert_eq!(entries.len(), 2);
//         assert_eq!(entries[0].index, 0);
//         assert_eq!(entries[0].term, 2);
//         assert_eq!(entries[0].command, "newcommand1");
//         assert_eq!(entries[1].index, 1);
//         assert_eq!(entries[1].term, 2);
//         assert_eq!(entries[1].command, "newcommand2");
//     }

//     #[test]
//     fn test_handle_request_vote_term_mismatch() {
//         let mut service = State::default();
//         service.current_term = 2;

//         let response = service.handle_request_vote(1, "127.0.0.1:4001", 0, 0);

//         assert_eq!(response.vote_granted(), false);
//         assert_eq!(response.term(), 2);
//         assert_eq!(service.current_term, 2);
//         assert_eq!(service.voted_for, None);
//         assert_eq!(service.role, Role::Follower);
//     }

//     #[test]
//     fn test_handle_request_vote_valid_vote_candidate() {
//         let mut service = State::default();
//         service.role = Role::Candidate;
//         service.log_entries = LogEntries(vec![LogEntry::new(0, 1, "command1".to_string())]);

//         let response = service.handle_request_vote(2, "127.0.0.1:4001", 0, 2);

//         assert_eq!(response.vote_granted(), true);
//         assert_eq!(response.term(), 2);
//         assert_eq!(service.current_term, 2);
//         assert_eq!(
//             service.voted_for,
//             Some(SocketAddr::from_str("127.0.0.1:4001").unwrap())
//         );
//         assert_eq!(service.role, Role::Follower);
//         assert_eq!(service.leader, None);
//     }

//     #[test]
//     fn test_handle_request_vote_valid_vote() {
//         let mut service = State::default();
//         service.log_entries = LogEntries(vec![LogEntry::new(0, 1, "command1".to_string())]);

//         let response = service.handle_request_vote(2, "127.0.0.1:4001", 1, 1);

//         assert_eq!(response.vote_granted(), true);
//         assert_eq!(response.term(), 2);
//         assert_eq!(service.current_term, 2);
//         assert_eq!(
//             service.voted_for,
//             Some(SocketAddr::from_str("127.0.0.1:4001").unwrap())
//         );
//         assert_eq!(service.role, Role::Follower);
//         assert_eq!(service.leader, None);
//     }

//     #[test]
//     fn test_handle_request_vote_already_voted() {
//         let mut service = State::default();
//         service.current_term = 2;
//         service.voted_for = Some(SocketAddr::from_str("127.0.0.1:4001").unwrap());
//         service.log_entries = LogEntries(vec![LogEntry::new(0, 1, "command1".to_string())]);

//         let response = service.handle_request_vote(2, "127.0.0.1:4002", 0, 1);

//         assert_eq!(response.vote_granted(), false);
//         assert_eq!(response.term(), 2);
//         assert_eq!(service.current_term, 2);
//         assert_eq!(
//             service.voted_for,
//             Some(SocketAddr::from_str("127.0.0.1:4001").unwrap())
//         );
//         assert_eq!(service.role, Role::Follower);
//     }

//     #[test]
//     fn test_handle_request_vote_logs_not_up_to_date() {
//         let mut service = State::default();
//         service.log_entries = LogEntries(vec![
//             LogEntry::new(0, 1, "command1".to_string()),
//             LogEntry::new(1, 2, "command1".to_string()),
//         ]);

//         let response = service.handle_request_vote(2, "127.0.0.1:4001", 1, 1);

//         assert_eq!(response.vote_granted(), false);
//         assert_eq!(response.term(), 2);
//         assert_eq!(service.current_term, 2);
//         assert_eq!(service.voted_for, None);
//     }
// }
