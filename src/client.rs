use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;
use std::net::SocketAddr;
use tokio::task::JoinSet;

use crate::{
    consensus::Consensus,
    dto::{AppendEntriesResponse, VoteResponse},
    peers::{Peers, PeersManagement},
    raft_capnp::{command, raft},
    storage::LogEntry,
};

struct RequestError {
    index: usize,
    error: capnp::Error,
}

pub async fn append_entries<S: Consensus + PeersManagement>(
    state: &S,
    peers: &Peers,
    entries: &[LogEntry],
) -> Result<SuccessfulAppendEntriesResult, FailureAppendEntriesResult> {
    let tasks = prepare_append_entries_tasks(state, peers, entries).await;
    manage_append_entries_tasks(tasks).await
}

async fn prepare_append_entries_tasks<S: Consensus + PeersManagement>(
    state: &S,
    peers: &Peers,
    entries: &[LogEntry],
) -> JoinSet<Result<AppendEntriesResponse, RequestError>> {
    let mut tasks = JoinSet::new();
    let current_term = state.current_term();
    let addr = state.id().addr().to_string();
    let commit_index = state.commit_index();
    let last_log_info = state.last_log_info();
    // let mut m = capnp::message::Builder::new_default();

    for (index, peer) in peers.iter().enumerate() {
        let client = peer.client();
        //TODO: avoid the loops and create the request n times

        let mut request = client.append_entries_request();

        let mut data = request.get().init_request();
        data.set_term(current_term);
        data.set_leader_id(&addr);
        data.set_prev_log_index(last_log_info.last_log_index());
        data.set_prev_log_term(last_log_info.last_log_term());
        data.set_leader_commit(commit_index);
        let mut entries_client = data.init_entries(entries.len() as u32);
        for (i, entry) in entries.iter().enumerate() {
            let mut entry_data = entries_client.reborrow().get(i as u32);
            entry_data.set_index(entry.index());
            entry_data.set_term(entry.term());
            entry_data.set_command(entry.command());
        }

        tasks.spawn_local(send_append_entries(request, index));
    }
    tasks
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
            tracing::error!(
                "error from evaluating the reply send_append_entries: {:?}",
                err
            );
            return Err(RequestError { index, error: err });
        }
    };

    let response_value = match response.get() {
        Ok(r) => r.get_response(),
        Err(err) => {
            tracing::error!(
                "error from send_append_entries getting the response: {:?}",
                err
            );
            return Err(RequestError { index, error: err });
        }
    };

    let response = match response_value {
        Ok(r) => r.try_into(),
        Err(err) => {
            tracing::error!(
                "error from send_append_entries getting the response: {:?}",
                err
            );
            return Err(RequestError { index, error: err });
        }
    };

    match response {
        Ok(r) => Ok(r),
        Err(err) => {
            tracing::error!(
                "error from send_append_entries getting the response: {:?}",
                err
            );
            Err(RequestError { index, error: err })
        }
    }
}

pub struct FailureAppendEntriesResult {
    higher_term: u64,
    failed_peers: Vec<usize>,
}
impl FailureAppendEntriesResult {
    pub fn higher_term(&self) -> u64 {
        self.higher_term
    }
    pub fn failed_peers(&self) -> &[usize] {
        &self.failed_peers
    }
}
pub struct SuccessfulAppendEntriesResult {
    appends_succesful: u64,
    failed_peers: Vec<usize>,
}
impl SuccessfulAppendEntriesResult {
    pub fn appends_succesful(&self) -> u64 {
        self.appends_succesful
    }
    pub fn failed_peers(&self) -> &[usize] {
        &self.failed_peers
    }
}

async fn manage_append_entries_tasks(
    mut tasks: JoinSet<Result<AppendEntriesResponse, RequestError>>,
) -> Result<SuccessfulAppendEntriesResult, FailureAppendEntriesResult> {
    let mut appends_succesful = 1;
    let mut failed_peers = Vec::with_capacity(tasks.len());

    while let Some(res) = tasks.join_next().await {
        let task_result = match res {
            Ok(response) => response,
            Err(err) => {
                tracing::error!("error in manage_append_entries_tasks: {:?}", err);
                continue;
            }
        };
        let append_entries_response = match task_result {
            Ok(response) => response,
            Err(error) => {
                failed_peers.push(error.index);
                continue;
            }
        };

        match append_entries_response {
            AppendEntriesResponse::Ok => appends_succesful += 1,
            AppendEntriesResponse::Err(higher_term) => {
                return Err(FailureAppendEntriesResult {
                    higher_term,
                    failed_peers,
                });
            }
        }
    }

    Ok(SuccessfulAppendEntriesResult {
        appends_succesful,
        failed_peers,
    })
}

pub async fn vote<S: Consensus + PeersManagement>(state: &S, peers: &Peers) -> VoteResult {
    let mut tasks = prepare_vote_tasks(state, peers).await;
    collect_vote_results(&mut tasks).await
}

async fn prepare_vote_tasks<S: Consensus + PeersManagement>(
    state: &S,
    peers: &Peers,
) -> JoinSet<Result<VoteResponse, RequestError>> {
    let mut tasks = JoinSet::new();
    let current_term = state.current_term();
    let addr = state.id().addr().to_string();
    let last_log_info = state.last_log_info();

    //TODO: probably should be better to use a select and hook the server to listen for the heartbeat
    // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html#examples
    for (index, peer) in peers.iter().enumerate() {
        let client = peer.client();

        let mut request = client.request_vote_request();
        request.get().set_term(current_term);
        request.get().set_candidate_id(&addr);
        request
            .get()
            .set_last_log_index(last_log_info.last_log_index());
        request
            .get()
            .set_last_log_term(last_log_info.last_log_term());

        tasks.spawn_local(send_vote(request, index));
    }
    tasks
}

async fn send_vote(
    request: capnp::capability::Request<
        raft::request_vote_params::Owned,
        raft::request_vote_results::Owned,
    >,
    index: usize,
) -> Result<VoteResponse, RequestError> {
    let reply = request.send().promise.await;

    let response = match reply {
        Ok(r) => r,
        Err(err) => {
            tracing::error!(
                "error from evaluating the reply send_vote_request: {:?}",
                err
            );
            return Err(RequestError { index, error: err });
        }
    };

    let response_value = match response.get() {
        Ok(r) => r.get_response(),
        Err(err) => {
            tracing::error!(
                "error from send_vote_request getting the response: {:?}",
                err
            );
            return Err(RequestError { index, error: err });
        }
    };

    match response_value {
        Ok(r) => Ok(r.into()),
        Err(err) => {
            tracing::error!(
                "error from send_vote_request getting the response: {:?}",
                err
            );
            Err(RequestError { index, error: err })
        }
    }
}

pub struct VoteResult {
    votes_granted: u64,
    failed_peers: Vec<usize>,
}

impl VoteResult {
    pub fn votes_granted(&self) -> u64 {
        self.votes_granted
    }

    pub fn failed_peers(&self) -> &[usize] {
        &self.failed_peers
    }
}

//TODO: get the errors when the term is lower than the one from the peer
async fn collect_vote_results(
    tasks: &mut JoinSet<Result<VoteResponse, RequestError>>,
) -> VoteResult {
    // We already “voted for ourselves,” so start at 1.
    let mut votes_granted = 1;
    let mut failed_peers = Vec::with_capacity(tasks.len());

    while let Some(joined) = tasks.join_next().await {
        match joined {
            // The task itself panicked or was cancelled (JoinError)
            Err(join_error) => {
                tracing::error!("JoinSet task failed: {:?}", join_error);
                // We don't know which peer index that was; skip
                continue;
            }

            // The task completed; now look at its Result<VoteResponse, RequestError>
            Ok(task_result) => match task_result {
                // RPC succeeded and gave us a VoteResponse
                Ok(vote_resp) => {
                    votes_granted += vote_resp.vote_granted() as u64;
                }
                // RPC errored (network failure, etc.)
                Err(req_err) => {
                    tracing::warn!(
                        "Peer {} had RPC failure: {:?}",
                        req_err.index,
                        req_err.error
                    );
                    failed_peers.push(req_err.index);
                }
            },
        }
    }

    VoteResult {
        votes_granted,
        failed_peers,
    }
}

pub async fn create_client(addr: &SocketAddr) -> Result<raft::Client, Box<dyn std::error::Error>> {
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

pub async fn create_client_com(
    addr: &SocketAddr,
) -> Result<command::Client, Box<dyn std::error::Error>> {
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
    let client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);

    tokio::task::spawn_local(rpc_system);
    Ok(client)
}
