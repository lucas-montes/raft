use std::{net::SocketAddr, str::FromStr};

use capnp::capability::Promise;
use capnp_rpc::pry;

use crate::{
    client::create_client,
    consensus::AppendEntriesResult,
    dto::{CommandMsg, RaftMsg},
    peers::{PeerDisconnected, PeersDisconnected},
    raft_capnp::raft,
    storage::LogEntry,
};

use super::Server;

impl raft::Server for Server {
    fn join_cluster(
        &mut self,
        params: raft::JoinClusterParams,
        _: raft::JoinClusterResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let request = pry!(params.get());

        pry!(request.get_history());
        //TODO: check that the peer can join, that the history is correct, etc...
        let peer = pry!(request.get_peer());

        // let client = pry!(peer.get_client());
        let addr = pry!(pry!(peer.get_address()).to_str());
        let id = pry!(pry!(peer.get_id()).to_str());

        let addr = SocketAddr::from_str(addr).expect("failed to parse NodeId from string");

        // let msg = Peer::new(id, addr, client);

        let msg = PeersDisconnected::new(vec![PeerDisconnected::new(id.into(), addr)].into_iter());

        tracing::info!(action = "requestJoinCluster", peer = %addr, id = %id);

        let channel = self.peers_channel.clone();
        Promise::from_future(async move {
            channel.send(msg).await.expect("msg not sent");
            Ok(())
        })
    }

    fn get_leader(
        &mut self,
        _: raft::GetLeaderParams,
        mut results: raft::GetLeaderResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let channel = self.commands_channel.clone();
        Promise::from_future(async move {
            let (msg, rx) = CommandMsg::get_leader();
            channel.send(msg).await.expect("msg not sent");
            if let Some(leader) = rx.await.expect("msg not received") {
                let client = create_client(&leader).await.expect("msg not received");
                results.get().set_leader(client);
            };
            Ok(())
        })
    }
    /// The node (a follower or candidate) receives a request from the leader to update its log
    fn append_entries(
        &mut self,
        params: raft::AppendEntriesParams,
        mut results: raft::AppendEntriesResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let request = pry!(pry!(params.get()).get_request());
        let entries = pry!(request.get_entries());
        //TODO: find a way to no use a list
        let mut new_entries: Vec<LogEntry> = Vec::with_capacity(entries.len() as usize);
        for e in entries {
            new_entries.push(LogEntry::new(
                e.get_index(),
                e.get_term(),
                pry!(e.get_command()).to_vec(),
            ));
        }
        let leader = pry!(pry!(request.get_leader_id()).to_str());

        let (msg, rx) = RaftMsg::request_append_entries(
            request.get_term(),
            leader.into(),
            request.get_prev_log_index(),
            request.get_prev_log_term(),
            new_entries,
            request.get_leader_commit(),
        );

        let raft_channel = self.raft_channel.clone();
        // let entries_client = pry!(request.get_handle_entries());

        Promise::from_future(async move {
            if let Err(err) = raft_channel.send(msg).await {
                tracing::error!("error sending the append_entries to the state {err}");
                return Err(capnp::Error::failed(
                    "error sending the append_entries to the state".into(),
                ));
            };

            match rx.await {
                Ok(resp) => {
                    let mut response = results.get().get_response()?;

                    match resp {
                        AppendEntriesResult::Ok => {
                            response.set_ok(());
                        }

                        AppendEntriesResult::TermMismatch(term) => {
                            response.set_err(term);
                        }
                        AppendEntriesResult::LogEntriesMismatch {
                            last_index: _,
                            last_term: _,
                        } => {
                            // let mut entries_client = entries_client.get_request();
                            // entries_client.get().set_last_log_index(last_index);
                            // entries_client.get().set_last_log_term(last_term);
                            // let entries_up_to_date = entries_client.send().promise.await?;
                            //TODO: use the response to update its log's entries

                            response.set_ok(());
                        }
                    }
                    Ok(())
                }
                Err(err) => {
                    tracing::error!("error receiving the append_entries response {err}");
                    Err(capnp::Error::failed(
                        "error receiving the append_entries response".into(),
                    ))
                }
            }
        })
    }

    /// The node (a follower or candidate) receives a request from an other candidate to vote for it
    fn request_vote(
        &mut self,
        params: raft::RequestVoteParams,
        mut results: raft::RequestVoteResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let request = pry!(params.get());
        let candidate_id = pry!(pry!(request.get_candidate_id()).to_string());
        let last_log_index = request.get_last_log_index();
        let last_log_term = request.get_last_log_term();
        let term = request.get_term();

        let (msg, rx) = RaftMsg::request_vote(term, candidate_id, last_log_index, last_log_term);
        let raft_channel = self.raft_channel.clone();

        Promise::from_future(async move {
            if let Err(err) = raft_channel.send(msg).await {
                tracing::error!("error sending the request_vote to the state {err}");
                return Err(capnp::Error::failed(
                    "error sending the request_vote to the state".into(),
                ));
            };

            match rx.await {
                Ok(resp) => {
                    let mut response = results.get().get_response()?;
                    response.set_vote_granted(resp.vote_granted());
                    response.set_term(resp.term());
                    Ok(())
                }
                Err(err) => {
                    tracing::error!("error receiving the request_vote response {err}");
                    Err(capnp::Error::failed(
                        "error receiving the request_vote response".into(),
                    ))
                }
            }
        })
    }
}
