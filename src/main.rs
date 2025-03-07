use std::net::{SocketAddr, ToSocketAddrs};

pub mod raft_capnp {
    include!(concat!(env!("OUT_DIR"), "/raft_capnp.rs"));
}
mod server;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<SocketAddr> = ::std::env::args()
        .skip(1)
        .flat_map(|f| f.to_socket_addrs().expect("wrong addresse"))
        .collect();

    server::serve(args[0], &args[1..]).await?;
    Ok(())
}
