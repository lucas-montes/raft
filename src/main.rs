use std::{net::{SocketAddr, ToSocketAddrs}, rc::Rc};

use concensus::{Node};
use server::Server;
use client::Client;

pub mod raft_capnp {
    include!(concat!(env!("OUT_DIR"), "/raft_capnp.rs"));
}
mod concensus;
mod server;
mod client;

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
            let  service = Node::new(addr);

let mut client = Client::new(service.clone());
            for node in nodes {
                client.add_peer(node).await;
            }


            let server_task = tokio::task::spawn_local(Server::new(service.clone()).run());
                let client_task = tokio::task::spawn_local(client.run());

                match tokio::try_join!(server_task, client_task) {
                    Ok(_) => {
                        println!("Server and client are running");
                    }
                    Err(err) => {
                        println!("Error in main: {:?}", err);
                }};
        })
        .await;

}
