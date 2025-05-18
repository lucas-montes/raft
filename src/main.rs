use std::net::SocketAddr;

use clap::Parser;
use rand::random_range;
use server::Server;
use state::{Node, NodeId, State};

pub mod raft_capnp {
    include!(concat!(env!("OUT_DIR"), "/raft_capnp.rs"));
}

mod client;
mod consensus;
mod dto;
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
    let cli = Cli::parse();

    let nodes: Vec<SocketAddr> = cli
        .nodes
        .into_iter()
        .filter(|p| !p.port().eq(&cli.addr.port()))
        .collect();

    tokio::task::LocalSet::new()
        .run_until(async move {
            let heartbeat_interval = random_range(1.0..2.9);
            let election_timeout = random_range(3.0..6.0);
            let state = State::new(NodeId::new(cli.addr.clone()));
            let (rtx, rrx) = tokio::sync::mpsc::channel(100);
            let (ctx, crx) = tokio::sync::mpsc::channel(100);
            let mut service = Node::new(state, heartbeat_interval, election_timeout, rrx, crx);

            let server = Server::new(rtx, ctx);

            let server_task = tokio::task::spawn_local(server.run(cli.addr));

            for node in nodes {
                service.add_peer(node).await;
            }

            let client_task = tokio::task::spawn_local(service.run());

            match tokio::try_join!(server_task, client_task) {
                Ok(_) => {
                    println!("Server and client are running");
                }
                Err(err) => {
                    println!("Error in main: {:?}", err);
                }
            };
        })
        .await;
}
