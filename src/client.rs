// use capnp::capability::Promise;
use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;
use std::{fmt::Debug, net::SocketAddr, time::Duration};
use tokio::task::JoinSet;

use crate::{
    consensus::Consensus,
    dto::{AppendEntriesResponse, VoteResponse},
    raft_capnp::{self, raft}, state::Peer,
};

type Err = Box<dyn std::error::Error>;

struct RequestError {
    index: usize,
    error: capnp::Error,
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

trait Client: Consensus where Self: Sized{
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

        let response = match response_value {
            Ok(r) => r.try_into(),
            Err(err) => {
                println!(
                    "error from send_append_entries getting the response: {:?}",
                    err
                );
                return Err(RequestError { index, error: err });
            }
        };

        match response {
            Ok(r) => Ok(r),
            Err(err) => {
                println!(
                    "error from send_append_entries getting the response: {:?}",
                    err
                );
                return Err(RequestError { index, error: err });
            }
        }
    }

    async fn manage_append_entries_tasks(
        &self,
        mut tasks: JoinSet<Result<AppendEntriesResponse, RequestError>>,
    ) {
        while let Some(res) = tasks.join_next().await {
            let task_result = match res {
                Ok(response) => response,
                Err(err) => {
                    println!("error in append_entries: {:?}", err);
                    continue;
                }
            };
            let append_entries_response = match task_result {
                Ok(response) => response,
                Err(error) => {
                    // self.peers[error.index].reconnect().await;
                    continue;
                }
            };

            if let AppendEntriesResponse::Err(term) = append_entries_response {
                //NOTE: maybe check if the term is lower? normally it's as the follower is validating it
                // self.become_follower(self.node.addr(), term);
                println!("im a follower now so i do not send more hearbeats");
                break;
            };
        }
    }

    async fn prepare_append_entries_tasks(mut self) {
        let mut tasks = JoinSet::new();
        let current_term = self.current_term();
        // let node_addr = self.state.addr().to_string();
        let commit_index = self.commit_index();
        let (last_term, last_index) = self.last_log_info();

        println!("sending heartbeats");

        for (index, peer) in  self.peers().iter().enumerate() {
            let client = peer.client();

            let mut request = client.append_entries_request();

            let mut data = request.get().init_request();
            // let mut leader = data.reborrow().init_leader();

            // data.set_term(current_term);

            // leader.set_id("value");
            // leader.set_address("value");
            // // leader.set_client(value);
            // data.set_leader(leader.into_reader());

            data.set_prev_log_index(last_index);
            data.set_prev_log_term(last_term);
            // data.set_leader_commit(commit_index);
            // data.init_entries(0);

            tasks.spawn_local(Self::send_append_entries(request, index));
        }
        //TODO: split in two, one to send, one to receive

        // while let Some(res) = tasks.join_next().await {
        //     let task_result = match res {
        //         Ok(response) => response,
        //         Err(err) => {
        //             println!("error in append_entries: {:?}", err);
        //             continue;
        //         }
        //     };
        //     let append_entries_response = match task_result {
        //         Ok(response) => response,
        //         Err(error) => {
        //             // self.peers[error.index].reconnect().await;
        //             continue;
        //         }
        //     };

        //     if let AppendEntriesResponse::Err(term) = append_entries_response {
        //         //NOTE: maybe check if the term is lower? normally it's as the follower is validating it
        //         // self.become_follower(self.node.addr(), term);
        //         println!("im a follower now so i do not send more hearbeats");
        //         break;
        //     };
        // }
    }

    // async fn append_entries(&mut self) {
    //     let tasks = self.prepare_append_entries_tasks().await;
    //     self.manage_append_entries_tasks(tasks).await;
    // }

    async fn vote(&mut self){
        let tasks = self.prepare_vote_tasks().await;
        self.manage_vote_tasks(tasks).await;
    }

    async fn send_vote(
        reply: Result<capnp::capability::Response<raft::request_vote_results::Owned>, capnp::Error>,
        index: usize,
    ) -> Result<VoteResponse, RequestError> {
        // let reply = request.send().promise.await;
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
    async fn manage_vote_tasks(&mut self, mut tasks: JoinSet<Result<capnp::capability::Response<raft::request_vote_results::Owned>, capnp::Error>>) {
        //NOTE: we start with one vote because we vote for ourself
        let mut votes = 1;
        while let Some(res) = tasks.join_next().await {
            Self::send_vote(reply, index)
            match res.expect("why joinhandle failed?") {
                Ok(r) => {
                    votes += r.vote_granted() as u64;
                }
                Err(error) => {
                    // self.peers[error.index].reconnect().await;
                }
            }
        }

        //NOTE: we add one to the number of nodes to count ourself
        let num_nodes = self.peers().len() + 1;
        let has_majority = votes > num_nodes.div_euclid(2) as u64;
        if has_majority || num_nodes.eq(&1) {
            self.become_leader()
        } else {
            self.become_candidate()
        }
    }

    async fn prepare_vote_tasks(&mut self) -> JoinSet<Result<capnp::capability::Response<raft::request_vote_results::Owned>, capnp::Error>>{
        let mut tasks = JoinSet::new();
        let current_term = self.current_term();
        // let addr = self.node.addr()();
        let (last_term, last_index) = self.last_log_info();

        //TODO: probably should be better to use a select and hook the server to listen for the heartbeat
        // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html#examples
        for (index, peer) in self.peers().iter().enumerate() {
            let client = peer.client();

            let mut request = client.request_vote_request();
            request.get().set_term(current_term);
            // request.get().set_candidate_id(&addr);
            request.get().set_last_log_index(last_index);
            request.get().set_last_log_term(last_term);
            let p = request.send().promise;
            // let t = Self::send_vote(p, index);

            tasks.spawn_local(p);
        }
        tasks
        // let mut votes = 1;
        // while let Some(res) = tasks.join_next().await {

        //     let res = match res {
        //         Ok(r) => {r
        //             // votes += r.vote_granted() as u64;
        //         }
        //         Err(error) => {
        //             // self.peers()[error.index].reconnect().await;
        //             return;
        //         }
        //     };
        //     Self::send_vote(res, 0).await;

        // }

        // //NOTE: we add one to the number of nodes to count ourself
        // let num_nodes = self.peers().len() + 1;
        // let has_majority = votes > num_nodes.div_euclid(2) as u64;
        // if has_majority || num_nodes.eq(&1) {
        //     self.become_leader()
        // } else {
        //     self.become_candidate()
        // }
    }
}
