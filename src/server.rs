use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;

use crate::{concensus::{Node}, raft_capnp::raft};


type Err = Box<dyn std::error::Error>;
pub struct Server {
    node: Node
}
impl Server {

    pub async fn run(self) -> Result<(), Err> {
        let listener = tokio::net::TcpListener::bind(&self.node.addr()).await?;
        let client: raft::Client = capnp_rpc::new_client(self);
        loop {
            let (stream, _) = listener.accept().await?;
            stream.set_nodelay(true)?;
            let (reader, writer) = tokio_util::compat::TokioAsyncReadCompatExt::compat(stream).split();
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
        Self {
            node
        }
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
        println!("from apprend_entries: current term: {t}");
        results.get().set_success(false);
        results.get().set_term(t + 1);
        Promise::ok(())
    }

    fn request_vote(
        &mut self,
        _: raft::RequestVoteParams,
        _: raft::RequestVoteResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        Promise::ok(())
    }
}
