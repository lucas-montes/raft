use std::{net::SocketAddr, rc::Rc};

use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;
use tokio::sync::mpsc::Sender;

use crate::{
    client::{create_client, create_client_com}, consensus::AppendEntriesResult, dto::{CommandMsg, RaftMsg}, raft_capnp::{command, raft}, state::{Role, State}, storage::LogEntry
};

#[derive(Debug, Clone)]
pub struct Server {
    raft_channel: Sender<RaftMsg>,
    commands_channel: Sender<CommandMsg>,
}

impl Server {
    pub async fn run(self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        println!("server start");
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

    pub fn new(raft_channel: Sender<RaftMsg>, commands_channel: Sender<CommandMsg>) -> Self {
        Self {
            raft_channel,
            commands_channel,
        }
    }
}

impl command::Server for Server {
    fn start_transaction(
        &mut self,
        params: command::StartTransactionParams,
        mut results: command::StartTransactionResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let channel = self.commands_channel.clone();
        Promise::from_future(async move {
            let (msg, rx) = CommandMsg::get_leader();
            channel.send(msg).await.expect("msg not sent");
            if let Some(leader) = rx.await.expect("msg not received"){
                let client = create_client_com(&leader).await.expect("msg not received");
                 results
                .get()
                .set_leader(client);};
            Ok(())
        })
    }
}

impl raft::Server for Server {
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
                pry!(e.get_command())
                    .to_string()
                    .expect("error getting command as string"),
            ));
        }
        let leader = pry!(request.get_leader_id());

        let (msg, rx) = RaftMsg::request_append_entries(
            request.get_term(),
            "leader".into(),
            request.get_prev_log_index(),
            request.get_prev_log_term(),
            new_entries,
            request.get_leader_commit(),
        );

        if let Err(err) = self.raft_channel.blocking_send(msg) {
            println!("error sending the append_entries to the state {err}");
            return Promise::err(capnp::Error::failed(
                "error sending the append_entries to the state".into(),
            ));
        };

        match rx.blocking_recv() {
            Ok(resp) => {
                let entries_client = pry!(request.get_handle_entries());

                //TODO: make it better
                Promise::from_future(async move {
                    let mut response = results.get().get_response()?;

                    match resp {
                        AppendEntriesResult::Ok => {
                            response.set_ok(());
                        }

                        AppendEntriesResult::TermMismatch(term) => {
                            response.set_err(term);
                        }
                        AppendEntriesResult::LogEntriesMismatch {
                            last_index,
                            last_term,
                        } => {
                            let mut entries_client = entries_client.get_request();
                            entries_client.get().set_last_log_index(last_index);
                            entries_client.get().set_last_log_term(last_term);
                            let entries_up_to_date = entries_client.send().promise.await?;
                            //TODO: use the response to update its log's entries

                            response.set_ok(());
                        }
                    }
                    Ok(())
                })
            }
            Err(err) => {
                println!("error receiving the append_entries response {err}");
                Promise::err(capnp::Error::failed(
                    "error receiving the append_entries response".into(),
                ))
            }
        }
    }

    /// The node (a follower or candidate) receives a request from an other candidate to vote for it
    fn request_vote(
        &mut self,
        params: raft::RequestVoteParams,
        mut results: raft::RequestVoteResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let request = pry!(params.get());
        println!("the server received the request_vote {request:?}");
        let candidate_id = pry!(pry!(request.get_candidate_id()).to_string());
        let last_log_index = request.get_last_log_index();
        let last_log_term = request.get_last_log_term();
        let term = request.get_term();

        let (msg, rx) = RaftMsg::request_vote(term, candidate_id, last_log_index, last_log_term);

        if let Err(err) = self.raft_channel.blocking_send(msg) {
            println!("error sending the request_vote to the state {err}");
            return Promise::err(capnp::Error::failed(
                "error sending the request_vote to the state".into(),
            ));
        };

        match rx.blocking_recv() {
            Ok(resp) => {
                let mut response = pry!(results.get().get_response());

                response.set_vote_granted(resp.vote_granted());
                response.set_term(resp.term());
                Promise::ok(())
            }
            Err(err) => {
                println!("error receiving the request_vote response {err}");
                Promise::err(capnp::Error::failed(
                    "error receiving the request_vote response".into(),
                ))
            }
        }
    }
}
