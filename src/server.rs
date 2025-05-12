use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;

use crate::{
    concensus::{LogEntry, Node},
    raft_capnp::raft,
};

type Err = Box<dyn std::error::Error>;

#[derive(Debug)]
pub struct Server {
    node: Node,
}

impl Server {
    pub async fn run(self) -> Result<(), Err> {
        println!("server start");
        let listener = tokio::net::TcpListener::bind(&self.node.addr()).await?;
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

    pub fn new(node: Node) -> Self {
        Self { node }
    }
}

impl raft::Server for Server {
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
        let leader_id = pry!(pry!(request.get_leader_id()).to_str());

        let resp = self.node.handle_append_entries(
            request.get_term(),
            leader_id,
            request.get_prev_log_index() as usize,
            request.get_prev_log_term(),
            request.get_leader_commit(),
            new_entries,
        );

        let mut response = pry!(results.get().get_response());
        response.set_success(resp.success());
        response.set_term(resp.term());
        Promise::ok(())
    }

    fn request_vote(
        &mut self,
        params: raft::RequestVoteParams,
        mut results: raft::RequestVoteResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let request = pry!(params.get());
        println!("the server received the request_vote {request:?}");
        let candidate_id = pry!(pry!(request.get_candidate_id()).to_str());
        let last_log_index = request.get_last_log_index();
        let last_log_term = request.get_last_log_term();
        let term = request.get_term();

        let resp = self
            .node
            .handle_request_vote(term, candidate_id, last_log_index, last_log_term);

        results.get().set_vote_granted(resp.vote_granted());
        results.get().set_term(resp.term());
        Promise::ok(())
    }
}
