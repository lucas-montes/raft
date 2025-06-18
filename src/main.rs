use std::{net::SocketAddr, path::PathBuf};

use clap::Parser;
use node::Node;
use peers::PeersReconnectionTask;
use rand::random_range;
use server::Server;
use state::{NodeId, State};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod raft_capnp {
    include!(concat!(env!("OUT_DIR"), "/raft_capnp.rs"));
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
    #[arg(long, default_value_t = 1.0)]
    min_heartbeat: f64,
    #[arg(long, default_value_t = 2.9)]
    max_heartbeat: f64,
    #[arg(long, default_value_t = 3.0)]
    min_election_timeout: f64,
    #[arg(long, default_value_t = 6.0)]
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
            let heartbeat_interval = random_range(cli.min_heartbeat..cli.max_heartbeat);
            let election_timeout = random_range(cli.min_election_timeout..cli.max_election_timeout);

            let (raft_tx, raft_rx) = tokio::sync::mpsc::channel(cli.raft_channel_size);
            let (commands_tx, commands_rx) = tokio::sync::mpsc::channel(cli.commands_channel_size);
            let (peers_tx, peers_rx) = tokio::sync::mpsc::channel(cli.peers_channel_size);
            let (peers_task_tx, peers_task_rx) =
                tokio::sync::mpsc::channel(cli.peers_task_channel_size);

            let state = State::new(NodeId::new(cli.addr), cli.state_path);
            let service = Node::new(
                state,
                heartbeat_interval,
                election_timeout,
                raft_rx,
                commands_rx,
                peers_rx,
                peers_task_tx.clone(),
            );
            let reconnection_task = PeersReconnectionTask::new(
                peers_task_rx,
                peers_tx.clone(),
                cli.peer_connection_backoff,
                cli.peer_connection_max_backoff,
            );
            let server = Server::new(raft_tx, commands_tx, peers_tx);

            let server_task = tokio::task::spawn_local(server.run(cli.addr));
            let client_task = tokio::task::spawn_local(service.run());
            let peers_task = tokio::task::spawn_local(reconnection_task.run());

            //Connect to peers in the cluster
            peers_task_tx
                .send(peers::PeersDisconnected::new(cli.nodes.into_iter().map(
                    |addr| peers::PeerDisconnected::new(NodeId::new(addr), addr),
                )))
                .await
                .expect("Failed to send initial peers");

            match tokio::try_join!(server_task, client_task, peers_task) {
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
