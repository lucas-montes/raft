use std::{net::SocketAddr, str::FromStr};

use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;
use tokio::sync::mpsc::Sender;

use crate::{
    client::{create_client, create_client_com},
    consensus::AppendEntriesResult,
    dto::{CommandMsg, RaftMsg},
    peers::{NewPeer, Peer},
    raft_capnp::{command, raft},
    storage::LogEntry,
};

#[derive(Debug, Clone)]
pub struct Server {
    raft_channel: Sender<RaftMsg>,
    commands_channel: Sender<CommandMsg>,
    peers_channel: Sender<NewPeer>,
}

impl Server {
    pub fn new(
        raft_channel: Sender<RaftMsg>,
        commands_channel: Sender<CommandMsg>,
        peers_channel: Sender<NewPeer>,
    ) -> Self {
        Self {
            raft_channel,
            commands_channel,
            peers_channel,
        }
    }

    pub async fn run(self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        let client: raft::Client = capnp_rpc::new_client(self);
        loop {
            let (stream, _) = listener.accept().await?;
            stream.set_nodelay(true)?;
            let (reader, writer) =
                tokio_util::compat::TokioAsyncReadCompatExt::compat(stream).split();
            let network = twoparty::VatNetwork::new(
                futures::io::BufReader::new(reader),
                futures::io::BufWriter::new(writer),
                rpc_twoparty_capnp::Side::Server,
                Default::default(),
            );

            let rpc_system = RpcSystem::new(Box::new(network), Some(client.clone().client));

            tokio::task::spawn_local(rpc_system);
        }
    }
}

struct Item {
    id: String,
    data: Vec<u8>, //TODO: make it a pointer or something
    commands_channel: Sender<CommandMsg>,
}

impl command::item::Server for Item {
    fn read(
        &mut self,
        _: command::item::ReadParams,
        mut results: command::item::ReadResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let mut data = results.get().init_data();
        data.set_data(&self.data);
        data.set_id(&self.id);
        Promise::ok(())
    }

    fn update(
        &mut self,
        params: command::item::UpdateParams,
        mut results: command::item::UpdateResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let data = pry!(pry!(params.get()).get_data()).to_vec();
        let commands_channel = self.commands_channel.clone();
        let id = self.id.clone();
        Promise::from_future(async move {
            let (msg, rx) = CommandMsg::update(id, data);
            if let Err(err) = commands_channel.send(msg).await {
                tracing::error!("error sending the create command {err}");
                return Err(capnp::Error::failed(
                    "error sending the create command".into(),
                ));
            };
            match rx.await {
                Ok(entry) => {
                    results.get().set_item(capnp_rpc::new_client(Item {
                        id: entry.id,
                        data: entry.data,
                        commands_channel,
                    }));
                }
                Err(err) => {
                    tracing::error!("error receiving the update command {err}");
                    return Err(capnp::Error::failed(
                        "error receiving the update command".into(),
                    ));
                }
            };

            Ok(())
        })
    }
}

impl command::Server for Server {
    fn start_transaction(
        &mut self,
        _: command::StartTransactionParams,
        mut results: command::StartTransactionResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let channel = self.commands_channel.clone();
        tracing::info!(action = "startTransaction");
        Promise::from_future(async move {
            let (msg, rx) = CommandMsg::get_leader();
            channel.send(msg).await.expect("msg not sent");
            if let Some(leader) = rx.await.expect("msg not received") {
                let client = create_client_com(&leader).await.expect("msg not received");
                results.get().set_leader(client);
            };
            Ok(())
        })
    }

    fn create(
        &mut self,
        params: command::CreateParams,
        mut results: command::CreateResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let commands_channel = self.commands_channel.clone();
        let data = pry!(pry!(params.get()).get_data()).to_vec();
        tracing::info!(action = "create");
        Promise::from_future(async move {
            let (msg, rx) = CommandMsg::create(data);
            if let Err(err) = commands_channel.send(msg).await {
                tracing::error!("error sending the create command {err}");
                return Err(capnp::Error::failed(
                    "error sending the create command".into(),
                ));
            };
            match rx.await {
                Ok(entry) => {
                    results.get().set_item(capnp_rpc::new_client(Item {
                        id: entry.id,
                        data: entry.data,
                        commands_channel,
                    }));
                }
                Err(err) => {
                    tracing::error!("error receiving the create command {err}");
                    return Err(capnp::Error::failed(
                        "error receiving the create command".into(),
                    ));
                }
            };
            Ok(())
        })
    }
}

impl raft::Server for Server {
    fn join_cluster(
        &mut self,
        params: raft::JoinClusterParams,
        _: raft::JoinClusterResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let request = pry!(params.get());

        pry!(request.get_history());
        let peer = pry!(request.get_peer());

        let client = pry!(peer.get_client());
        let addr = pry!(pry!(peer.get_address()).to_str());
        let id = pry!(pry!(peer.get_id()).to_str());

        let addr = SocketAddr::from_str(addr).expect("failed to parse NodeId from string");

        let msg = Peer::new(id, addr, client);

        let channel = self.peers_channel.clone();
        Promise::from_future(async move {
            channel.send(msg.into()).await.expect("msg not sent");
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
