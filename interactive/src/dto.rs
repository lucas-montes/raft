use serde::{Deserialize, Serialize};
use tokio::{
    sync::{
        RwLock,
        mpsc::{self, Sender, error::SendError},
    },
    task::LocalSet,
};

use crate::client::handle_request;

#[derive(Clone)]
pub struct LocalSpawner {
    pub send: mpsc::UnboundedSender<CrudMessage>,
}

impl LocalSpawner {
    pub fn new() -> Self {
        let (send, mut recv) = mpsc::unbounded_channel();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        std::thread::spawn(move || {
            let local = LocalSet::new();

            local.spawn_local(async move {
                while let Some(new_task) = recv.recv().await {
                    tokio::task::spawn_local(handle_request(new_task));
                }
                // If the while loop returns, then all the LocalSpawner
                // objects have been dropped.
            });

            // This will return once all senders are dropped and all
            // spawned tasks have returned.
            rt.block_on(local);
        });

        Self { send }
    }

    pub fn spawn(&self, task: CrudMessage) -> Result<(), SendError<CrudMessage>> {
        self.send.send(task)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum CrudOperation {
    Create,
    Read,
    Update,
    Delete,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CrudRequest {
    pub operation: CrudOperation,
    pub id: Option<String>,
    pub data: Option<String>,
}

pub struct CrudMessage {
    pub request: CrudRequest,
    pub nodes: Vec<String>,
}
