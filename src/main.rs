use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;
use client::create_client;
use consensus::Consensus;
use node::Node;
use peers::{PeersDisconnected, PeersReconnectionTask};
use raft_capnp::raft;
use rand::{rngs::StdRng, Rng, SeedableRng};
use server::Server;
use state::{NodeId, State};
use storage::LogsInformation;
use tokio::sync::mpsc::Sender;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod raft_capnp {
    include!(concat!(env!("OUT_DIR"), "/raft_capnp.rs"));
}

pub mod storage_capnp {
    include!(concat!(env!("OUT_DIR"), "/storage_capnp.rs"));
}

mod client;
mod consensus;
mod dto;
mod node;
mod peers;
mod server;
mod state;
mod storage;

#[derive(Debug, Parser)]
pub struct Cli {
    /// Address to bind the server to
    #[arg(short, long)]
    addr: SocketAddr,
    /// Addresses of other nodes in the cluster
    #[arg(short, long, num_args = 0..)]
    nodes: Vec<SocketAddr>,
    /// Path where the hard state will be saved
    #[arg(short, long, default_value = "data/state")]
    state_path: PathBuf,
    #[arg(long, default_value_t = 3.0)]
    min_heartbeat: f64,
    #[arg(long, default_value_t = 5.9)]
    max_heartbeat: f64,
    #[arg(long, default_value_t = 6.0)]
    min_election_timeout: f64,
    #[arg(long, default_value_t = 8.0)]
    max_election_timeout: f64,
    /// Size of the channels used for communication between the server and the state
    #[arg(long, default_value_t = 100)]
    raft_channel_size: usize,
    /// Size of the channels used for communication between the server and the state related to database queries
    #[arg(long, default_value_t = 100)]
    commands_channel_size: usize,

    #[arg(long, default_value_t = 100)]
    peers_channel_size: usize,
    #[arg(long, default_value_t = 100)]
    peers_task_channel_size: usize,

    #[arg(long, default_value_t = 100)]
    peer_connection_backoff: u64,
    #[arg(long, default_value_t = 300)]
    peer_connection_max_backoff: u64,

    #[arg(long, default_value_t = 62984295692)]
    seed: u64,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // let file_appender = tracing_appender::rolling::hourly("/some/directory", "prefix.log");
    // let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .json()
                // .with_writer(non_blocking)
                .log_internal_errors(true)
                .with_target(false)
                .flatten_event(true)
                .with_span_list(false),
        )
        .init();

    let cli = Cli::parse();

    let span = tracing::span!(tracing::Level::INFO, "rafty", addr = %cli.addr);
    let _enter = span.enter();

    tokio::task::LocalSet::new()
        .run_until(async move {
            let mut rng = StdRng::seed_from_u64(cli.seed);

            let heartbeat_interval = rng.random_range(cli.min_heartbeat..cli.max_heartbeat);
            let election_timeout = Box::new(move || {
                rng.random_range(cli.min_election_timeout..cli.max_election_timeout)
            });

            let (raft_tx, raft_rx) = tokio::sync::mpsc::channel(cli.raft_channel_size);
            let (commands_tx, commands_rx) = tokio::sync::mpsc::channel(cli.commands_channel_size);
            let (peers_tx, peers_rx) = tokio::sync::mpsc::channel(cli.peers_channel_size);
            let (peers_task_tx, peers_task_rx) =
                tokio::sync::mpsc::channel(cli.peers_task_channel_size);

            let server = Server::new(raft_tx, commands_tx, peers_task_tx.clone());

            let server_task = tokio::task::spawn_local(server.run(cli.addr));

            let state = State::new(NodeId::new(cli.addr), cli.state_path);
            let log_info = state.last_log_info();

            let service = Node::new(
                state,
                heartbeat_interval,
                election_timeout,
                raft_rx,
                commands_rx,
                peers_rx,
                peers_task_tx.clone(),
            );

            let node_task = tokio::task::spawn_local(service.run());

            let reconnection_task = PeersReconnectionTask::new(
                peers_task_rx,
                peers_tx,
                cli.peer_connection_backoff,
                cli.peer_connection_max_backoff,
            );
            let peers_task = tokio::task::spawn_local(reconnection_task.run());

            let mut nodes = cli.nodes;
            nodes.retain(|addr| addr != &cli.addr);

            join_cluster(cli.addr, log_info, nodes, peers_task_tx).await;

            match tokio::try_join!(server_task, node_task, peers_task) {
                Ok(_) => {
                    tracing::info!("Server and client are running");
                }
                Err(err) => {
                    tracing::error!(err=?&err, "Error in main");
                }
            };
        })
        .await;
}

async fn join_cluster(
    addr: SocketAddr,
    log_info: LogsInformation,
    nodes: Vec<SocketAddr>,
    peers_task_tx: Sender<PeersDisconnected>,
) {
    //TODO: maybe make this a loop that tries to join the cluster until it succeeds
    if nodes.is_empty() {
        //No nodes provided, running as a single node
        return;
    }

    let mut count = 0;

    loop {
        for cluster_addr in &nodes {
            let cluster_client = match create_client(&cluster_addr).await {
                Ok(client) => client,
                Err(err) => {
                    tracing::error!(error = ?err, "Failed to create client for cluster address");
                    continue;
                }
            };

            let mut req = cluster_client
                .get_leader_request()
                .send()
                .pipeline
                .get_leader()
                .join_cluster_request();
            let mut history = req.get().init_history();
            history.set_last_log_index(log_info.last_log_index());
            history.set_last_log_term(log_info.last_log_term());

            let addr = addr.to_string();
            let mut peer = req.get().init_peer();
            peer.set_address(&addr);
            peer.set_id(&addr);
            // peer.set_client(client);

            match req.send().promise.await {
                Ok(response) => match response.get() {
                    Ok(response) => {
                        if let Ok(peers) = response.get_peers() {
                            peers_task_tx
                                .send(peers::PeersDisconnected::new(peers.iter().map(|addr| {
                                    peers::PeerDisconnected::new(
                                        NodeId::new(
                                            addr.get_id()
                                                .unwrap()
                                                .to_str()
                                                .unwrap()
                                                .parse()
                                                .unwrap(),
                                        ),
                                        addr.get_address()
                                            .unwrap()
                                            .to_str()
                                            .unwrap()
                                            .parse()
                                            .unwrap(),
                                    )
                                })))
                                .await
                                .expect("Failed to receive initial peers");
                        };
                        //TODO: if on peers then oupsi
                        tracing::info!(action = "clusterJoined");
                        return;
                    }
                    Err(err) => {
                        tracing::error!(err = ?err, "Failed to join cluster: {:?}", &cluster_addr);
                        continue;
                    }
                },
                Err(err) => {
                    tracing::error!(err = ?err, "Failed to join cluster: {:?}", &cluster_addr);
                    continue;
                }
            };
        }
        count += 1;
        if count >= 5 {
            tracing::error!("Failed to join cluster after {} attempts", count);
            panic!("Failed to join cluster after multiple attempts");
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(count)).await;
    }
}
