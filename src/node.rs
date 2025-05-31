use std::time::Duration;

use tokio::{
    sync::mpsc::{Receiver, Sender},
    time::{interval, sleep, Instant},
};

use crate::{
    client::{append_entries, vote},
    consensus::Consensus,
    dto::{CommandMsg, Entry, Operation, RaftMsg},
    peers::{NewPeer, PeersDisconnected, PeersManagement},
    state::Role,
};

#[derive(Debug)]
pub struct Node<S: Consensus + PeersManagement> {
    state: S,
    heartbeat_interval: f64,
    election_timeout: f64,
    raft_channel: Receiver<RaftMsg>,
    commands_channel: Receiver<CommandMsg>,
    peers_channel: Receiver<NewPeer>,
    peers_task_channel: Sender<PeersDisconnected>,
}

impl<S: Consensus + PeersManagement> Node<S> {
    pub fn new(
        state: S,
        heartbeat_interval: f64,
        election_timeout: f64,
        raft_channel: Receiver<RaftMsg>,
        commands_channel: Receiver<CommandMsg>,
        peers_channel: Receiver<NewPeer>,
        peers_task_channel: Sender<PeersDisconnected>,
    ) -> Self {
        Self {
            state,
            raft_channel,
            heartbeat_interval,
            election_timeout,
            commands_channel,
            peers_channel,
            peers_task_channel,
        }
    }
    pub fn state(&self) -> &S {
        &self.state
    }

    async fn handle_command(&mut self, rpc: CommandMsg) {
        match rpc {
            CommandMsg::GetLeader(req) => {
                let resp = self.state.leader();
                //TODO: instead of sendig only the addr and creating the client on the server I
                //should send the client in peers
                let sender = req.sender;
                if sender.send(resp).is_err() {
                    tracing::error!("Failed to send response in channel for get leader");
                }
            }
            CommandMsg::Read(_req) => {
                // Handle read command
            }
            CommandMsg::Modify(req) => {
                //TODO: when new operation:
                // 1 save into the in memory logs
                // 2 persist to disk
                // 3 broadcast to all peers
                // 4 when majority of peers have the log, apply to state machine
                let id = self.state.id().addr().to_string();
                let current_term = self.state.current_term();
                let log_entries = self.state.log_entries();
                match req.msg {
                    Operation::Create(data) => {
                        //TODO: it would be cool to be able to serialize the whole command? how to
                        //keep track of the commands in a better and easier way without copying all
                        //the data so much?
                        log_entries.new_entry(current_term, data.clone());
                        self.state.commit_hard_state().await;

                        let log_entries = self.state.log_entries();
                        if let Err(err) = log_entries.create(data.clone(), id.as_str()).await {
                            tracing::error!("Failed to create log entry {}", err);
                        };
                        let sender = req.sender;
                        let entry = Entry {
                            id: "hey".to_string(),
                            data,
                        };
                        if sender.send(entry).is_err() {
                            tracing::error!("Failed to send response in channel for create");
                        }
                    }
                    Operation::Update(_id, _data) => {
                        // Handle update command
                    }
                    Operation::Delete(_id) => {
                        // Handle delete command
                    }
                }
            }
        }
    }

    pub async fn run(mut self) {
        let heartbeat_dur = Duration::from_secs_f64(self.heartbeat_interval);
        let election_dur = Duration::from_secs_f64(self.election_timeout);
        let mut heartbeat_interval = interval(heartbeat_dur);
        let mut election_timeout = Box::pin(sleep(election_dur));

        let last_log_info = self.state.last_log_info();
        tracing::info!(action="starting", term=%self.state.current_term(), last_log_index=last_log_info.last_log_index(), last_log_term=last_log_info.last_log_term());

        loop {
            tokio::select! {
                Some(rpc) = self.peers_channel.recv() => {
                    // self.state.add_peer(rpc);
                }

                //  Incoming RPCs from external users
                Some(rpc) = self.commands_channel.recv() => {
                    self.handle_command(rpc).await;
                }

                Some(rpc) = self.raft_channel.recv() => {
                    election_timeout.as_mut().reset(Instant::now() + election_dur);
                    match rpc {
                        RaftMsg::AppendEntries(req) => {
                            let msg = req.msg;
                            let sender = req.sender;
                            let resp = self.state.handle_append_entries(
                                msg.term,
                                &msg.leader_id,
                                msg.prev_log_index as usize,
                                msg.prev_log_term,
                                msg.leader_commit,
                                msg.entries,
                            );
                            if sender.send(resp).is_err(){
                                tracing::error!("Failed to send response in channel for append entries");
                            }
                        }
                        RaftMsg::Vote(req) => {
                            let msg = req.msg;
                            let sender = req.sender;
                            let resp = self.state.handle_request_vote(
                                msg.term(),
                                msg.candidate_id(),
                                msg.last_log_index(),
                                msg.last_log_term(),
                            );
                            if sender.send(resp).is_err(){
                                tracing::error!("Failed to send response in channel for vote");
                            }
                        }
                    }
                }

                //TODO: maybe the following functions could be driven by the role and a trait

                //  election timeout fires → start election
                _ = &mut election_timeout, if self.state.role() != &Role::Leader => {
                    election_timeout.as_mut().reset(Instant::now() + election_dur);
                    tracing::info!(action="becomeCandidate");
                    self.state.become_candidate();
                    tracing::info!(action="sendVotes");
                    let result = vote(&self.state, self.state.peers()).await;
                }

                //  heartbeat tick → send heartbeats if leader
                _ = heartbeat_interval.tick(), if self.state.role() == &Role::Leader => {
                    tracing::info!(action="sendHeartbeat");
                    let result = append_entries(&self.state, self.state.peers(), &[]).await;

                    match result {
                        Ok(r) => {
                            let disco_peers = r.failed_peers().iter().map(|&i| self.state.remove_peer(i));
                            let disco = PeersDisconnected::new(disco_peers);
                            self.peers_task_channel.send(disco);
                        }
                        Err(err) => {
                            //NOTE: maybe check if the term is lower? normally it's as the follower is validating it
                            self.state.become_follower(None, err.higher_term());
                            tracing::info!(action = "becomeFollower", term = err.higher_term());
                        }
                    }

                }
            }
        }
    }
}
