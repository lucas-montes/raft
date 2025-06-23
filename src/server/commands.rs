use capnp::capability::Promise;
use capnp_rpc::pry;

use tokio::sync::mpsc::Sender;

use crate::{client::create_client_com, dto::CommandMsg, storage_capnp::command};

use super::Server;

struct Item {
    id: String,
    data: Vec<u8>, //TODO: make it a pointer or something
    commands_channel: Sender<CommandMsg>,
}

impl command::item::Server for Item {
    fn delete(
        &mut self,
        _: command::item::DeleteParams,
        _: command::item::DeleteResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let commands_channel = self.commands_channel.clone();
        let id: String = self.id.clone();
        tracing::info!(action = "delete");
        Promise::from_future(async move {
            let (msg, rx) = CommandMsg::delete(id);
            if let Err(err) = commands_channel.send(msg).await {
                tracing::error!(err = ?err, "error sending the delete command");
                return Err(capnp::Error::failed(
                    "error sending the delete command".into(),
                ));
            };
            match rx.await {
                Ok(_) => Ok(()),
                Err(err) => {
                    tracing::error!(err = ?err, "error receiving the delete command");
                    Err(capnp::Error::failed(
                        "error receiving the delete command".into(),
                    ))
                }
            }
        })
    }
    fn read(
        &mut self,
        _: command::item::ReadParams,
        mut results: command::item::ReadResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let mut data = results.get().init_data();
        tracing::info!(action = "read");
        data.set_data(&self.data);
        data.set_id(&self.id);
        Promise::ok(())
    }

    fn update(
        &mut self,
        params: command::item::UpdateParams,
        mut results: command::item::UpdateResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let data = pry!(pry!(params.get()).get_data()).to_vec();
        let commands_channel = self.commands_channel.clone();
        let id = self.id.clone();
        tracing::info!(action = "update");
        Promise::from_future(async move {
            let (msg, rx) = CommandMsg::update(id, data);
            if let Err(err) = commands_channel.send(msg).await {
                tracing::error!(err = ?err, "error sending the create command");
                return Err(capnp::Error::failed(
                    "error sending the create command".into(),
                ));
            };
            match rx.await {
                Ok(entry) => {
                    results.get().set_item(capnp_rpc::new_client(Item {
                        id: entry.id,
                        data: entry.data,
                        commands_channel,
                    }));
                }
                Err(err) => {
                    tracing::error!(err = ?err, "error receiving the update command");
                    return Err(capnp::Error::failed(
                        "error receiving the update command".into(),
                    ));
                }
            };

            Ok(())
        })
    }
}

impl command::Server for Server {
    fn start_transaction(
        &mut self,
        _: command::StartTransactionParams,
        mut results: command::StartTransactionResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let channel = self.commands_channel.clone();
        tracing::info!(action = "startTransaction");
        Promise::from_future(async move {
            let (msg, rx) = CommandMsg::get_leader();
            channel.send(msg).await.expect("msg not sent");
            if let Some(leader) = rx.await.expect("msg not received") {
                let client = create_client_com(&leader).await.expect("msg not received");
                results.get().set_leader(client);
            };
            Ok(())
        })
    }

    fn get(
        &mut self,
        params: command::GetParams,
        mut results: command::GetResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        results.get().set_item(capnp_rpc::new_client(Item {
            id: pry!(pry!(pry!(params.get()).get_id()).to_string()),
            data: vec![],
            commands_channel: self.commands_channel.clone(),
        }));
        Promise::ok(())
    }

    fn create(
        &mut self,
        params: command::CreateParams,
        mut results: command::CreateResults,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        let commands_channel = self.commands_channel.clone();
        let data = pry!(pry!(params.get()).get_data()).to_vec();
        tracing::info!(action = "create");
        Promise::from_future(async move {
            let (msg, rx) = CommandMsg::create(data);
            if let Err(err) = commands_channel.send(msg).await {
                tracing::error!(err = ?err, "error sending the create command");
                return Err(capnp::Error::failed(
                    "error sending the create command".into(),
                ));
            };
            match rx.await {
                Ok(entry) => {
                    results.get().set_item(capnp_rpc::new_client(Item {
                        id: entry.id,
                        data: entry.data,
                        commands_channel,
                    }));
                }
                Err(err) => {
                    tracing::error!(err = ?err, "error receiving the create command");
                    return Err(capnp::Error::failed(
                        "error receiving the create command".into(),
                    ));
                }
            };
            Ok(())
        })
    }
}
