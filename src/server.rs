use std::{net::SocketAddr, str::FromStr};

use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;

use crate::{concensus::Node, raft_capnp::raft};

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
        let request = pry!(params.get());
        let t = request.get_term();
        let entries = pry!(request.get_entries());
        if entries.is_empty() {
            //NOTE: just a heartbeat
            self.node.update_heartbeat()
        } else {
            //NOTE: actual entry to save
            println!("from apprend_entries: current term: {t}");
            results.get().set_success(false);
            results.get().set_term(t + 1);
        }
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

        let current_term = self.node.current_term();
        let term_is_uptodate = request.get_term() > current_term;

        //NOTE: use something better for the id of the server
        let condidate_id_matches = self.node.voted_for().is_none_or(|addr| {
            addr.eq(&SocketAddr::from_str(candidate_id)
                .expect("why candidate_id isnt a correct socketaddrs?"))
        });

        let (last_term, last_index) = self.node.last_log_info();
        let logs_uptodate =
            request.get_last_log_term() >= last_term && request.get_last_log_index() >= last_index;

        let vote_granted = term_is_uptodate && condidate_id_matches && logs_uptodate;

        results.get().set_vote_granted(vote_granted);
        results.get().set_term(current_term);
        Promise::ok(())
    }
}
