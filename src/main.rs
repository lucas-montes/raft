use std::{net::SocketAddr, rc::Rc, time::Duration};

use clap::Parser;
use rand::random_range;
use server::Server;
use state::{Node, NodeId, State};

pub mod raft_capnp {
    include!(concat!(env!("OUT_DIR"), "/raft_capnp.rs"));
}

mod client;
mod consensus;
mod storage;
mod dto;
mod state;
mod server;

fn get_election_timeout() -> Duration {
    let election_timeout = random_range(3.0..5.0);
    Duration::from_secs_f64(election_timeout)
}

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
            let latency = random_range(1.0..2.9);
            let mut state = State::new(NodeId::new(cli.addr));
            let (tx, rx) = tokio::sync::mpsc::channel(100);
            let mut service = Node::new(&mut state, latency, rx);

            let server = Server::new(Rc::new(state), tx);

            let server_task = tokio::task::spawn_local(server.run());

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
