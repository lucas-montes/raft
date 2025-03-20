use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;
use std::{fmt::Debug, net::SocketAddr, time::Duration};
use tokio::task::JoinSet;

use crate::{
    concensus::{Node, Role},
    raft_capnp::raft,
};

type Err = Box<dyn std::error::Error>;

pub enum Peer {
    Up {
        addr: SocketAddr,
        client: raft::Client,
    },
    Down(SocketAddr),
}

impl Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Peer::Up { addr, .. } => {
                write!(f, "Peer: {:?} is up", addr)
            }
            Peer::Down(addr) => {
                write!(f, "Peer: {:?} is down", addr)
            }
        }
    }
}

impl Peer {
    async fn new(addr: SocketAddr) -> Self {
        let mut counter = 0;
        loop {
            match create_client(&addr).await {
                Ok(client) => {
                    break Self::Up { addr, client };
                }
                Err(err) => {
                    println!("error in add_follower: {:?}", err);
                    counter += 1;
                    if counter > 15 {
                        break Self::Down(addr);
                    }
                    tokio::time::sleep(Duration::from_secs(1 + counter)).await;
                }
            }
        }
    }
}

async fn create_client(addr: &SocketAddr) -> Result<raft::Client, Err> {
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

#[derive(Debug)]
pub struct Client {
    node: Node,
    peers: Vec<Peer>,
}

impl Client {
    pub async fn run(mut self) -> Result<(), Err> {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            println!("client running, in state {:?}", self);
            match &self.node.role() {
                Role::Leader { .. } => self.leader_stage().await,
                Role::Follower => self.follower_stage().await,
                Role::Candidate => self.candidate_stage().await,
            }
        }
    }

    pub fn new(node: Node) -> Self {
        Self {
            node,
            peers: Vec::new(),
        }
    }
}

impl Client {
    pub async fn add_peer(&mut self, addr: SocketAddr) {
        self.peers.push(Peer::new(addr).await);
    }

    async fn send_messages(&mut self) {
        //        for peer in self.peers.iter_mut() {
        //            match peer {
        //                Peer::Up { client, .. } => {
        //                    let mut request = client.append_entries_request();
        //                    request.get().set_term(self.node.current_term());
        //                    let reply = request.send().promise.await;
        //                    match reply {
        //                        Ok(r) => {
        //                            println!("from send_messages: {:?}", r.get());
        //                        }
        //                        Err(err) => {
        //                            println!("from send_messages: {:?}", err);
        //                            if err.kind == capnp::ErrorKind::Disconnected {
        //                                println!("from send_messages: {:?}", err);
        //                            }
        //                        }
        //                    }
        //                }
        //                Peer::Down(addr) => {
        //                    println!("peer is down: {:?}", addr);
        //                }
        //            }
        //        }
    }

    async fn leader_stage(&mut self) {
        self.send_messages().await;
    }
    async fn follower_stage(&self) {}

    async fn send_vote_request(
        mut request: capnp::capability::Request<
            raft::request_vote_params::Owned,
            raft::request_vote_results::Owned,
        >,
        current_term: u64,
        addr: String,
        client_addr: SocketAddr,
        last_term: u64,
        last_index: u64,
        index: usize,
    ) -> Result<(u64, u64), (usize, SocketAddr)> {
        request.get().set_term(current_term);
        request.get().set_candidate_id(addr);
        request.get().set_last_log_index(last_index);
        request.get().set_last_log_term(last_term);
        let reply = request.send().promise.await;
        println!("sending vote");

        match reply {
            Ok(r) => match r.get() {
                Ok(response) => Ok((response.get_term(), response.get_vote_granted().into())),
                Err(err) => {
                    println!(
                        "error from send_vote_request getting the response: {:?}",
                        err
                    );
                    Err((index, client_addr))
                }
            },
            Err(err) => {
                println!(
                    "error from evaluating the reply send_vote_request: {:?}",
                    err
                );
                Err((index, client_addr))
                //    if err.kind == capnp::ErrorKind::Disconnected {
            }
        }
    }

    async fn candidate_stage(&mut self) {
        let mut tasks = JoinSet::new();
        let current_term = self.node.current_term();
        let node_addr = self.node.addr().to_string();
        let (last_term, last_index) = self.node.last_log_info();

        //TODO: probably should be better to use a select and hook the server to listen for the heartbeat
        // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html#examples
        for (index, peer) in self.peers.iter().enumerate() {
            match peer {
                Peer::Up { client, addr } => {
                    let request = client.request_vote_request();

                    tasks.spawn_local(Self::send_vote_request(
                        request,
                        current_term,
                        node_addr.clone(),
                        *addr,
                        last_term,
                        last_index,
                        index,
                    ));
                }
                Peer::Down(addr) => {
                    println!("peer is down: {:?}", addr);
                }
            }
        }
        let mut votes = 0;
        while let Some(res) = tasks.join_next().await {
            if let Ok(r) = res.expect("why joinhandle failed?") {
                votes += r.1;
            }
        }
        if votes > self.peers.len().div_ceil(2) as u64 {
            self.node.make_leader()
        }
    }
}
