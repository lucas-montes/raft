mod commands;
mod raft;

use std::net::SocketAddr;

use capnp_rpc::{rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;
use tokio::sync::mpsc::Sender;

use crate::{
    dto::{CommandMsg, RaftMsg},
    peers::PeersDisconnected,
    raft_capnp,
};

#[derive(Debug, Clone)]
pub struct Server {
    raft_channel: Sender<RaftMsg>,
    commands_channel: Sender<CommandMsg>,
    peers_channel: Sender<PeersDisconnected>,
}

impl Server {
    pub fn new(
        raft_channel: Sender<RaftMsg>,
        commands_channel: Sender<CommandMsg>,
        peers_channel: Sender<PeersDisconnected>,
    ) -> Self {
        Self {
            raft_channel,
            commands_channel,
            peers_channel,
        }
    }

    pub async fn run(self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        let client: raft_capnp::raft::Client = capnp_rpc::new_client(self);
        loop {
            let (stream, _) = listener.accept().await?;
            stream.set_nodelay(true)?;
            let (reader, writer) =
                tokio_util::compat::TokioAsyncReadCompatExt::compat(stream).split();
            let network = twoparty::VatNetwork::new(
                futures::io::BufReader::new(reader),
                futures::io::BufWriter::new(writer),
                rpc_twoparty_capnp::Side::Server,
                Default::default(),
            );

            let rpc_system = RpcSystem::new(Box::new(network), Some(client.clone().client));

            tokio::task::spawn_local(rpc_system);
        }
    }
}
