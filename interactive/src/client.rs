use std::net::SocketAddr;

use capnp_rpc::{RpcSystem, rpc_twoparty_capnp, twoparty};
use futures::AsyncReadExt;

use crate::{
    dto::{CrudMessage, CrudOperation},
    storage_capnp,
};

async fn create_client_com(
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

pub async fn handle_request(msg: CrudMessage) {
    let addr: SocketAddr = msg
        .nodes
        .first()
        .expect("No nodes available")
        .parse()
        .expect("invalid address");
    let client = create_client_com(&addr).await.expect("client failed");
    let start_transaction = client.start_transaction_request();
    let leader_client = start_transaction.send().pipeline.get_leader();

    let req = msg.request;

    match req.operation {
        CrudOperation::Create => {
            let value = req.data.unwrap();
            let value = value.as_bytes();
            let mut cluster_req = leader_client.create_request();
            cluster_req.get().set_data(value);
            let resp = cluster_req.send().pipeline.get_item();
            let resp = resp.read_request().send().promise.await.unwrap();
        }
        CrudOperation::Read | CrudOperation::Update | CrudOperation::Delete => {
            let mut cluster_req = leader_client.get_request();
            match req.operation {
                CrudOperation::Read => {
                    cluster_req.get().set_id(req.id.unwrap());
                    let resp = cluster_req.send().pipeline.get_item();
                    let resp = resp.read_request().send().promise.await.unwrap();
                }
                CrudOperation::Update => {
                    cluster_req.get().set_id(req.id.unwrap());
                    let value = req.data.unwrap();
                    let value = value.as_bytes();
                    let resp = cluster_req.send().pipeline.get_item();
                    let mut update_request = resp.update_request();
                    update_request.get().set_data(value);
                    let resp = update_request.send().promise.await.unwrap();
                }
                CrudOperation::Delete => {
                    cluster_req.get().set_id(req.id.unwrap());
                    let resp = cluster_req.send().pipeline.get_item();
                    let resp = resp.delete_request().send().promise.await.unwrap();
                }
                _ => unreachable!(),
            }
        }
    }
}

async fn execute() {}

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
