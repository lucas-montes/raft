use axum::{
    Router,
    extract::{
        State,
        connect_info::ConnectInfo,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::any,
};
use serde::{Deserialize, Serialize};

use tokio::{
    io::AsyncBufReadExt,
    process::{Child, Command},
    sync::{
        RwLock,
        mpsc::{self, Sender},
    },
};

use futures::{sink::SinkExt, stream::StreamExt};
use std::{collections::HashMap, net::SocketAddr, path::PathBuf, process::Stdio, sync::Arc};
use tower_http::services::ServeDir;

use crate::dto::{CrudMessage, CrudRequest, LocalSpawner};

#[derive(Clone)]
struct ClusterState {
    nodes: Arc<RwLock<HashMap<SocketAddr, NodeInfo>>>,
    front_transmission: Option<Sender<ServerLogType>>,
    spawner: LocalSpawner,
}

impl ClusterState {
    fn new(spawner: LocalSpawner) -> Self {
        ClusterState {
            nodes: Arc::new(RwLock::new(HashMap::new())),
            front_transmission: None,
            spawner,
        }
    }

    async fn get_nodes(&self) -> Vec<String> {
        self.nodes
            .read()
            .await
            .keys()
            .map(|addr| addr.to_string())
            .collect()
    }
}

struct NodeInfo {
    process: Child,
}

impl NodeInfo {
    fn new(process: Child) -> Self {
        NodeInfo { process }
    }
}

pub async fn main(addr: &SocketAddr, spawner: LocalSpawner) {
    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");

    let state = ClusterState::new(spawner);

    let app = Router::new()
        .fallback_service(ServeDir::new(assets_dir).append_index_html_on_directories(true))
        .route("/ws", any(ws_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<ClusterState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, addr, state))
}

async fn handle_socket(socket: WebSocket, who: SocketAddr, mut state: ClusterState) {
    let (mut socket_sender, mut socket_receiver) = socket.split();

    let (tx, mut rx) = mpsc::channel(100);
    state.front_transmission = Some(tx);

    tokio::spawn(async move {
        while let Some(Ok(msg)) = socket_receiver.next().await {
            if let Message::Text(text) = msg {
                match serde_json::from_str::<FrontendCommand>(&text) {
                    Ok(cmd) => match cmd {
                        FrontendCommand::AddNode { addr, port } => {
                            spawn_node(port, addr, &state).await;
                        }
                        FrontendCommand::RemoveNode { id } => {
                            remove_node(id.parse().expect("addr should be valid"), &state).await;
                        }
                        FrontendCommand::CrudOperation(req) => {
                            if let Err(e) = state.spawner.spawn(CrudMessage {
                                request: req,
                                nodes: state.get_nodes().await,
                            }) {
                                eprintln!("Error sending CRUD request: {:?}", e);
                            }
                        }
                    },
                    Err(e) => {
                        println!("Error parsing command: {:?}", e);
                    }
                }
            }
        }
    });

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let msg = match serde_json::to_string(&msg) {
                Ok(json) => json,
                Err(e) => {
                    eprintln!("Error serializing message: {:?}", e);
                    continue;
                }
            };

            if let Err(err) = socket_sender.send(msg.into()).await {
                eprintln!("Error sending message to socket: {:?}", err);
            }
        }
    });
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "action", rename_all = "camelCase")]
enum FrontendCommand {
    AddNode { addr: String, port: String },
    RemoveNode { id: String },
    CrudOperation(CrudRequest),
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerLogSpan {
    addr: SocketAddr,
    name: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerLog {
    timestamp: String,
    span: ServerLogSpan,
}

#[derive(Serialize, Deserialize, Debug)]
struct ServerLogError {
    message: String,
    err: String,
    #[serde(flatten)]
    data: ServerLog,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
enum ServerLogType {
    Starting(ServerLog),
    SendAppendEntries(ServerLog),
    SendHeartbeat(ServerLog),
    SendVotes(ServerLog),
    StartTransaction(ServerLog),
    VotingFor(ServerLog),
    ReceiveAppendEntries(ServerLog),
    ReceiveVote(ServerLog),
    BecomeCandidate(ServerLog),
    BecomeFollower(ServerLog),
    BecomeLeader(ServerLog),
    CreateHardState(ServerLog),
    PeerReconnectedSuccessfully(ServerLog),
    RequestJoinCluster(ServerLog),
    ClusterJoined(ServerLog),
    Read(ServerLog),
    Update(ServerLog),
    Delete(ServerLog),
    Create(ServerLog),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "UPPERCASE")]
enum ServerLogLevel {
    Error(ServerLogError),
    Info(ServerLogType),
}

async fn spawn_node(port: String, address: String, state: &ClusterState) {
    let full_addr = format!("{}:{}", address, port);
    let mut cmd = Command::new("../target/debug/raft")
        .arg("--addr")
        .arg(&full_addr)
        .arg("--nodes")
        .args(state.get_nodes().await)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = cmd.stdout.take().unwrap();
    let rx = state.front_transmission.clone().unwrap();
    tokio::spawn(async move {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            match serde_json::from_str::<ServerLogLevel>(&line) {
                Ok(cmd) => match cmd {
                    ServerLogLevel::Error(log) => {
                        eprintln!("Error log: {:?}", log);
                    }
                    ServerLogLevel::Info(log) => {
                        if let Err(err) = rx.send(log).await {
                            eprintln!("Error sending message to frontend");
                        }
                    }
                },
                Err(e) => {
                    eprintln!("Error parsing log: {:?} ; {:?}", e, line);
                }
            }
        }
    });

    state.nodes.write().await.insert(
        full_addr.parse().expect("the address should be valid"),
        NodeInfo::new(cmd),
    );
}

async fn remove_node(addr: SocketAddr, state: &ClusterState) {
    let mut nodes = state.nodes.write().await;
    if let Some(mut node) = nodes.remove(&addr) {
        node.process.kill().await.unwrap();
    }
}
