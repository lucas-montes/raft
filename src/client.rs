use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;
use std::net::SocketAddr;
use tokio::task::JoinSet;

use crate::{
    consensus::Consensus,
    dto::{AppendEntriesResponse, VoteResponse},
    raft_capnp::{command, raft},
    storage::LogEntry,
};

struct RequestError {
    index: usize,
    error: capnp::Error,
}

pub async fn append_entries<S: Consensus>(state: &mut S, entries: &[LogEntry]) {
    let tasks = prepare_append_entries_tasks(state, entries).await;
    manage_append_entries_tasks(state, tasks).await;
}

async fn prepare_append_entries_tasks<S: Consensus>(
    state: &mut S,
    entries: &[LogEntry],
) -> JoinSet<Result<AppendEntriesResponse, RequestError>> {
    let mut tasks = JoinSet::new();
    let current_term = state.current_term();
    let addr = state.id().addr().to_string();
    let commit_index = state.commit_index();
    let last_log_info = state.last_log_info();
    // let mut m = capnp::message::Builder::new_default();

    for (index, peer) in state.peers().iter().enumerate() {
        let client = peer.client();

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
            entry_data.set_command(&entry.command());
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
            return Err(RequestError { index, error: err });
        }
    }
}

async fn manage_append_entries_tasks<S: Consensus>(
    state: &mut S,
    mut tasks: JoinSet<Result<AppendEntriesResponse, RequestError>>,
) {
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
                state.restart_peer(error.index).await;
                continue;
            }
        };

        if let AppendEntriesResponse::Err(term) = append_entries_response {
            //NOTE: maybe check if the term is lower? normally it's as the follower is validating it
            state.become_follower(None, term);
            tracing::info!(action="become_follower", term = term);
            break;
        };
    }
}

pub async fn vote<S: Consensus>(state: &mut S) {
    let tasks = prepare_vote_tasks(state).await;
    manage_vote_tasks(state, tasks).await;
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

    return match response_value {
        Ok(r) => Ok(r.into()),
        Err(err) => {
            tracing::error!(
                "error from send_vote_request getting the response: {:?}",
                err
            );
            return Err(RequestError { index, error: err });
        }
    };
}

async fn prepare_vote_tasks<S: Consensus>(
    state: &mut S,
) -> JoinSet<Result<VoteResponse, RequestError>> {
    let mut tasks = JoinSet::new();
    let current_term = state.current_term();
    let addr = state.id().addr().to_string();
    let last_log_info = state.last_log_info();

    //TODO: probably should be better to use a select and hook the server to listen for the heartbeat
    // https://docs.rs/tokio/latest/tokio/task/struct.JoinSet.html#examples
    for (index, peer) in state.peers().iter().enumerate() {
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

async fn manage_vote_tasks<S: Consensus>(
    state: &mut S,
    mut tasks: JoinSet<Result<VoteResponse, RequestError>>,
) {
    //NOTE: we start with one vote because we vote for ourself
    let mut votes = 1;
    while let Some(res) = tasks.join_next().await {
        //TODO: make it better, extract the request creation so it can be reused and a new task added to the list if it fails
        match res.expect("why joinhandle failed?") {
            Ok(r) => {
                votes += r.vote_granted() as u64;
            }
            Err(error) => {
                //TODO: we hang here, it blocks. spawn it
                state.restart_peer(error.index).await;
            }
        }
    }
    state.count_votes(votes);
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
