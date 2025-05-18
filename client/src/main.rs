use std::net::SocketAddr;

use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use clap::Parser;
use futures::AsyncReadExt;
use rand::random_range;

pub mod raft_capnp {
    include!(concat!(env!("OUT_DIR"), "/raft_capnp.rs"));
}

#[derive(Debug, Parser)]
pub struct Cli {
    #[arg(short, long)]
    addr: SocketAddr,
}

pub async fn create_client_com(
    addr: &SocketAddr,
) -> Result<raft_capnp::command::Client, Box<dyn std::error::Error>> {
    let stream = tokio::net::TcpStream::connect(addr).await?;
    stream.set_nodelay(true)?;
    let (reader, writer) = tokio_util::compat::TokioAsyncReadCompatExt::compat(stream).split();
    let rpc_network = Box::new(twoparty::VatNetwork::new(
        futures::io::BufReader::new(reader),
        futures::io::BufWriter::new(writer),
        rpc_twoparty_capnp::Side::Client,
        Default::default(),
    ));
    let mut rpc_system = RpcSystem::new(rpc_network, None);
    let client = rpc_system.bootstrap(rpc_twoparty_capnp::Side::Server);

    tokio::task::spawn_local(rpc_system);
    Ok(client)
}

async fn request(cli: Cli) {
    let client = create_client_com(&cli.addr).await.expect("client failed");
    let req = client.start_transaction_request();
    let leader_client = req.send().pipeline.get_leader();

    let mut req = leader_client.create_request();

    let value = "{\"key\": \"value\"}".as_bytes();
    req.get().set_data(value);

    let resp = req.send().pipeline.get_item();
    let resp = resp.read_request().send().promise.await.unwrap();
    let raw_result = resp.get().unwrap().get_data().unwrap();
    let result = raw_result.get_data().unwrap();
    println!("response: {:?}", String::from_utf8_lossy(result));
    println!("response: {:?}", raw_result);
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();

    tokio::task::LocalSet::new()
        .run_until(request(cli))
        .await;
}
