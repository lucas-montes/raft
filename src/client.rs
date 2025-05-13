// use capnp::capability::Promise;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;
use std::{fmt::Debug, net::SocketAddr, time::Duration};
use tokio::task::JoinSet;

use crate::{
    concensus::{Node, Role},
    dto::{AppendEntriesResponse, RequestVoteResponse},
    raft_capnp::raft,
};

type Err = Box<dyn std::error::Error>;

pub struct Peer {
    addr: SocketAddr,
    client: raft::Client,
}

impl Debug for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer {{ addr: {:?} }}", self.addr)
    }
}

impl Peer {
    async fn new(addr: SocketAddr) -> Self {
        let client = Self::connect(&addr).await;
        Self { addr, client }
    }

    async fn reconnect(&mut self) {
        let client = Self::connect(&self.addr).await;
        self.client = client;
    }

    async fn connect(addr: &SocketAddr) -> raft::Client {
        let mut counter = 0;
        loop {
            match create_client(addr).await {
                Ok(client) => {
                    break client;
                }
                Err(err) => {
                    println!("error in add_follower: {:?}", err);
                    counter += 1;
                    tokio::time::sleep(Duration::from_secs(counter)).await;
                }
            }
        }
    }
}

struct RequestError {
    index: usize,
    error: capnp::Error,
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
        loop {
            match &self.node.role().await {
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

    async fn send_append_entries(
        request: capnp::capability::Request<
            raft::append_entries_params::Owned,
            raft::append_entries_results::Owned,
        >,
        index: usize,
    ) -> Result<AppendEntriesResponse, RequestError> {
        let reply = request.send().promise.await;

        let response = match reply {
            Ok(r) => r,
            Err(err) => {
                println!(
                    "error from evaluating the reply send_append_entries: {:?}",
                    err
                );
                return Err(RequestError { index, error: err });
            }
        };

        let response_value = match response.get() {
            Ok(r) => r.get_response(),
            Err(err) => {
                println!(
                    "error from send_append_entries getting the response: {:?}",
                    err
                );
                return Err(RequestError { index, error: err });
            }
        };

        return match response_value {
            Ok(r) => Ok(r.into()),
            Err(err) => {
                println!(
                    "error from send_append_entries getting the response: {:?}",
                    err
                );
                return Err(RequestError { index, error: err });
            }
        };
    }

    async fn leader_stage(&mut self) {
        let mut tasks = JoinSet::new();
        let current_term = self.node.current_term();
        let node_addr = self.node.addr().to_string();
        let commit_index = self.node.commit_index();
        let (last_term, last_index) = self.node.last_log_info();

        tokio::time::sleep(Duration::from_secs_f64(self.node.heartbeat_latency())).await;
        println!("sending heartbeats");
        for (index, peer) in self.peers.iter().enumerate() {
            let client = &peer.client;

            let mut request = client.append_entries_request();
            let mut data = request.get().init_request();
            data.set_term(current_term);
            data.set_leader_id(&node_addr);
            data.set_prev_log_index(last_index);
            data.set_prev_log_term(last_term);
            data.set_leader_commit(commit_index);
            data.init_entries(0);

            tasks.spawn_local(Self::send_append_entries(request, index));
        }

        while let Some(res) = tasks.join_next().await {
            match res {
                Ok(response) => match response {
                    Ok(response) => {
                        if response.term() > self.node.current_term() {
                            self.node
                                .make_follower(Some(self.node.addr()), response.term());
                            println!("im a follower now so i do not send more hearbeats");
                            break;
                        }
                    }
                    Err(error) => {
                        self.peers[error.index].reconnect().await;
                    }
                },
                Err(err) => {
                    println!("error in append_entries: {:?}", err);
                }
            };
        }
    }

    async fn follower_stage(&self) {
        tokio::time::sleep(Duration::from_millis(25)).await
    }

    async fn send_vote_request(
        request: capnp::capability::Request<
            raft::request_vote_params::Owned,
            raft::request_vote_results::Owned,
        >,

        index: usize,
    ) -> Result<RequestVoteResponse, RequestError> {
        let reply = request.send().promise.await;
        println!("sending vote");

        let response = match reply {
            Ok(r) => r,
            Err(err) => {
                println!(
                    "error from evaluating the reply send_vote_request: {:?}",
                    err
                );
                return Err(RequestError { index, error: err });
            }
        };

        let response_value = match response.get() {
            Ok(r) => r.get_response(),
            Err(err) => {
                println!(
                    "error from send_vote_request getting the response: {:?}",
                    err
                );
                return Err(RequestError { index, error: err });
            }
        };

        return match response_value {
            Ok(r) => Ok(r.into()),
            Err(err) => {
                println!(
                    "error from send_vote_request getting the response: {:?}",
                    err
                );
                return Err(RequestError { index, error: err });
            }
        };
    }

    async fn candidate_stage(&mut self) {
        let mut tasks = JoinSet::new();
        let current_term = self.node.current_term();
        let addr = self.node.addr().to_string();
        let (last_term, last_index) = self.node.last_log_info();

        tokio::time::sleep(Duration::from_secs_f64(self.node.heartbeat_latency())).await;
        //TODO: probably should be better to use a select and hook the server to listen for the heartbeat
        // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html#examples
        for (index, peer) in self.peers.iter().enumerate() {
            let client = &peer.client;

            let mut request = client.request_vote_request();
            request.get().set_term(current_term);
            request.get().set_candidate_id(&addr);
            request.get().set_last_log_index(last_index);
            request.get().set_last_log_term(last_term);

            tasks.spawn_local(Self::send_vote_request(request, index));
        }
        //NOTE: we start with one vote because we vote for ourself
        let mut votes = 1;
        while let Some(res) = tasks.join_next().await {
            match res.expect("why joinhandle failed?") {
                Ok(r) => {
                    votes += r.vote_granted() as u64;
                }
                Err(error) => {
                    self.peers[error.index].reconnect().await;
                }
            }
        }

        //NOTE: we add one to the number of nodes to count ourself
        let num_nodes = self.peers.len() + 1;
        let has_majority = votes > num_nodes.div_euclid(2) as u64;
        if has_majority || num_nodes.eq(&1) {
            self.node.make_leader()
        } else {
            self.node.make_candidate()
        }
    }
}
