use std::{
    net::{SocketAddr, ToSocketAddrs},
    time::Duration,
};

use clap::Parser;
use client::Client;
use concensus::Node;
use rand::random_range;
use server::Server;

pub mod raft_capnp {
    include!(concat!(env!("OUT_DIR"), "/raft_capnp.rs"));
}


mod client;
mod concensus;
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
            let mut service = Node::new(cli.addr, latency);
            let server = Server::new(service.clone());

            let server_task = tokio::task::spawn_local(server.run());

            let mut client = Client::new(service.clone());
            for node in nodes {
                client.add_peer(node).await;
            }

            let client_task = tokio::task::spawn_local(client.run());
            let election_task = tokio::task::spawn_local(async move {
                let mut election_timeout = get_election_timeout();
                let mut counter = 0;
                println!("election loop, {:?}", election_timeout);
                loop {
                    tokio::time::sleep(election_timeout).await;
                    service.check_election(election_timeout);
                    counter += 1;
                    if counter % 3 == 0 {
                        election_timeout = get_election_timeout();
                    }
                }
            });

            match tokio::try_join!(server_task, client_task, election_task) {
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
