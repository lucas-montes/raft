use clap::{Parser, ValueEnum};
use std::net::SocketAddr;

mod client;
mod server;

pub mod storage_capnp {
    include!(concat!(env!("OUT_DIR"), "/storage_capnp.rs"));
}

#[derive(Clone, ValueEnum)]
enum Side {
    Client,
    Server,
}

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "127.0.0.1:3000")]
    addr: SocketAddr,
    #[arg(short, long, default_value = "server")]
    side: Side,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.side {
        Side::Client => client::main(&cli.addr).await,
        Side::Server => server::main(&cli.addr).await,
    }

    // tokio::task::LocalSet::new()
    //     .run_until(request(&cli.addr))
    //     .await;
}
