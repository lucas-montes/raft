use std::{ fmt::Debug, net::SocketAddr, time::Duration};
use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;

use crate::{concensus::{Node,  Role}, raft_capnp::raft};


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
                    if counter > 5 {
                        break Self::Down(addr);
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
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

pub struct Client {
    node: Node,
    peers: Vec<Peer>,
}



impl Client {
    pub async fn run(mut self) -> Result<(), Err> {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            println!("running");
            match &self.node.role() {
                Role::Leader{..} => self.leader_stage().await,
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
        for peer in self.peers.iter_mut() {
            match peer {
                Peer::Up { client, .. } => {
                    let mut request = client.append_entries_request();
                    request.get().set_term(self.node.current_term());
                    let reply = request.send().promise.await;
                    match reply {
                        Ok(r) => {
                            println!("from send_messages: {:?}", r.get());
                        }
                        Err(err) => {
                            println!("from send_messages: {:?}", err);
                            match err.kind {
                                capnp::ErrorKind::Disconnected => {
                                    println!("from send_messages: {:?}", err);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Peer::Down(addr) => {
                    println!("peer is down: {:?}", addr);
                }
            }
        }
    }

    async fn leader_stage(&mut self) {

        self.send_messages().await;

    }
    async fn follower_stage(&self) {}
    async fn candidate_stage(&self) {}
}
