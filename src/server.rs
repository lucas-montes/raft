use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};

use crate::raft_capnp::raft;

use futures::AsyncReadExt;
use std::{net::SocketAddr, time::Duration};

type Err = Box<dyn std::error::Error>;

#[derive(Default)]
enum State {
    Leader,
    Candidate,
    #[default]
    Follower,
}

#[derive(Default)]
struct Service {
    id: u64,
    state: State,
    leaders: Vec<raft::Client>,
    followers: Vec<raft::Client>,
    term: u64,
}

impl Service {
    async fn add_follower(&mut self, addr: &SocketAddr) -> Result<(), Err> {
        let client = create_client(addr).await?;
        self.followers.push(client);
        Ok(())
    }

    async fn run(self) -> Result<(), Err> {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            match &self.state {
                State::Leader => self.leader_stage().await,
                State::Follower => self.follower_stage().await,
                State::Candidate => self.candidate_stage().await,
            }
        }
    }

    async fn leader_stage(&self) {
        for node in &self.followers {
            match send_req(node, self.term).await {
                Ok(r) => {println!("reponser in leader reque {:?}",r)}
                Err(err) => println!("error in leader requ{:?}", err),
            };
        }
    }
    async fn follower_stage(&self) {}
    async fn candidate_stage(&self) {}
}

struct RaftImpl;

impl raft::Server for RaftImpl {
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
}

pub async fn create_client(addr: &SocketAddr) -> Result<raft::Client, Err> {
    let stream = tokio::net::TcpStream::connect(addr).await?;
    stream.set_nodelay(true)?;
    let (reader, writer) = tokio_util::compat::TokioAsyncReadCompatExt::compat(stream).split();
    let rpc_network = Box::new(twoparty::VatNetwork::new(
        futures::io::BufReader::new(reader),
        futures::io::BufWriter::new(writer),
        rpc_twoparty_capnp::Side::Client,
        Default::default(),
    ));
    let mut rpc_system = RpcSystem::new(rpc_network, None);
    let client: raft::Client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);

    tokio::task::spawn_local(rpc_system);
    Ok(client)
}

async fn send_req(client: &raft::Client, term: u64) -> Result<(), Err> {
    let mut request = client.append_entries_request();
    request.get().set_term(term);

    let reply = request.send().promise.await?;

    println!(
        "from send_req, received: {} {}",
        reply.get()?.get_term(),
        reply.get()?.get_success()
    );
    Ok(())
}

async fn start_communication(addr: SocketAddr, nodes: Vec<SocketAddr>) -> Result<(), Err> {
    let mut service = Service::default();
    if addr.port().eq(&4000) {
        service.state = State::Leader;
    }
    for node in nodes {
        service.add_follower(&node).await?;
    }
    service.run().await
}

pub async fn serve(addr: SocketAddr, nodes: &[SocketAddr]) -> Result<(), Err> {
    println!("current: {:?}, nodes: {:?}", &addr, nodes);
    tokio::task::LocalSet::new()
        .run_until(async move {
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            let client: raft::Client = capnp_rpc::new_client(RaftImpl);

            tokio::task::spawn_local(start_communication(addr, nodes.to_vec()));
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
        })
        .await
}
