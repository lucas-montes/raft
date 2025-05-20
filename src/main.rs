use std::net::SocketAddr;

use clap::Parser;
use node::Node;
use rand::random_range;
use server::Server;
use state::{NodeId, State};
use tracing_subscriber::{fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt};

pub mod raft_capnp {
    include!(concat!(env!("OUT_DIR"), "/raft_capnp.rs"));
}

mod client;
mod consensus;
mod dto;
mod node;
mod server;
mod state;
mod storage;

#[derive(Debug, Parser)]
pub struct Cli {
    #[arg(short, long)]
    addr: SocketAddr,
    #[arg(short, long, num_args = 1..)]
    nodes: Vec<SocketAddr>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    //     let file_appender = tracing_appender::rolling::hourly("/some/directory", "prefix.log");
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

    let nodes: Vec<SocketAddr> = cli
        .nodes
        .into_iter()
        .filter(|p| !p.port().eq(&cli.addr.port()))
        .collect();

    tokio::task::LocalSet::new()
        .run_until(async move {
            let heartbeat_interval = random_range(1.0..2.9);
            let election_timeout = random_range(3.0..6.0);
            //TODO: when starting a new node (or restarting) read the state from disk

            let (rtx, rrx) = tokio::sync::mpsc::channel(100);
            let (ctx, crx) = tokio::sync::mpsc::channel(100);

            let server = Server::new(rtx, ctx);
            let server_task = tokio::task::spawn_local(server.run(cli.addr));

            let mut state = State::new(NodeId::new(cli.addr.clone()));

            for node in nodes {
                state.add_peer(node).await;
            }
            let service = Node::new(state, heartbeat_interval, election_timeout, rrx, crx);

            let client_task = tokio::task::spawn_local(service.run());

            match tokio::try_join!(server_task, client_task) {
                Ok(_) => {
                    tracing::info!("Server and client are running");
                }
                Err(err) => {
                    tracing::error!("Error in main: {:?}", err);
                }
            };
        })
        .await;
}
