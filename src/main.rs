use std::{
    net::{SocketAddr, ToSocketAddrs},
    time::Duration,
};

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
    let election_timeout = random_range(150..300);
    Duration::from_millis(election_timeout)
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<SocketAddr> = ::std::env::args()
        .skip(1)
        .flat_map(|f| f.to_socket_addrs().expect("wrong addresse"))
        .collect();

    let addr = args[0];
    let nodes = args[1..].to_vec();

    tokio::task::LocalSet::new()
        .run_until(async move {
            let mut service = Node::new(addr);

            let server_task = tokio::task::spawn_local(Server::new(service.clone()).run());

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
