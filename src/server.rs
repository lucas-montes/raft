use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};

use crate::raft_capnp::raft;

use futures::AsyncReadExt;
use std::{net::SocketAddr, time::Duration};

type Err = Box<dyn std::error::Error>;

#[derive(Default)]
enum Role {
    Leader,
    Candidate,
    #[default]
    Follower,
}

struct LogEntry {
    term: u64,
    msg: String,
}

struct State {
    current_term: u64,
    voted_for: Option<u64>,
    log_entries: Vec<LogEntry>,
}

#[derive(Default)]
struct Service {
    id: u64,
    role: Role,
    followers: Vec<raft::Client>,
    term: u64,
}

impl Service {
    async fn new(addr: SocketAddr, nodes: Vec<SocketAddr>) -> Result<Self, Err> {
        let mut service = Service::default();
        if addr.port().eq(&4000) {
            println!("im the leader boys");
            service.role = Role::Leader;
        }
        for node in nodes {
            //TODO: it seems that if a client cannot connect everything breaks silently
            service.add_follower(&node).await?;
        }
        Ok(service)
    }

    async fn add_follower(&mut self, addr: &SocketAddr) -> Result<(), Err> {
        let client = create_client(addr).await?;
        self.followers.push(client);
        Ok(())
    }

    async fn run(mut self) -> Result<(), Err> {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            println!("running");
            match &self.role {
                Role::Leader => self.leader_stage().await,
                Role::Follower => self.follower_stage().await,
                Role::Candidate => self.candidate_stage().await,
            }
        }
    }

    async fn leader_stage(&mut self) {
        let term = self.term;
        let mut terms = [term; 2];
        for (i, node) in self.followers.iter().enumerate() {
            match send_req(node, term).await {
                Ok(r) => {
                    println!("reponser in leader reque {:?}", r);
                    terms[i] = r;
                }
                Err(err) => println!("error in leader requ{:?}", err),
            };
        }
        let first = terms.first().expect("no term? wierd");
        if terms.iter().all(|x| x.eq(first)) {
            self.term = *first;
        } else {
            println!("couldnt update the temr")
        }
    }
    async fn follower_stage(&self) {}
    async fn candidate_stage(&self) {}
}

struct Node {}

impl raft::Server for Node {
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

async fn send_req(client: &raft::Client, term: u64) -> Result<u64, Err> {
    let mut request = client.append_entries_request();
    request.get().set_term(term);

    let reply = request.send().promise.await?;
    let new_term = reply.get()?.get_term();

    println!(
        "from send_req, received term: {} success: {}",
        new_term,
        reply.get()?.get_success()
    );
    Ok(new_term)
}

async fn strart_service(addr: SocketAddr, nodes: Vec<SocketAddr>) -> Result<(), Err> {
    let s = Service::new(addr, nodes)
        .await
        .expect("service oculdnt start");
    s.run().await
}

pub async fn serve(addr: SocketAddr, nodes: &[SocketAddr]) -> Result<(), Err> {
    println!("current: {:?}, nodes: {:?}", &addr, nodes);
    tokio::task::LocalSet::new()
        .run_until(async move {
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            let client: raft::Client = capnp_rpc::new_client(Node {});
            tokio::task::spawn_local(strart_service(addr, nodes.to_vec()));
            println!("started the bg task {:?}", &addr);
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
