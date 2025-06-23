use std::net::SocketAddr;

use capnp_rpc::{RpcSystem, rpc_twoparty_capnp, twoparty};
use futures::AsyncReadExt;

use crate::storage_capnp;

pub async fn create_client_com(
    addr: &SocketAddr,
) -> Result<storage_capnp::command::Client, Box<dyn std::error::Error>> {
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

pub async fn main(addr: &SocketAddr) {
    let client = create_client_com(addr).await.expect("client failed");
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
