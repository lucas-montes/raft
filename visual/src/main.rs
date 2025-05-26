use axum::{
    Router,
    extract::{
        State,
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

use std::{collections::HashMap, process::Stdio, sync::Arc};
use std::{net::SocketAddr, path::PathBuf};
use tower_http::services::ServeDir;

//allows to extract the IP of connecting user
use axum::extract::connect_info::ConnectInfo;

//allows to split the websocket stream into separate TX and RX branches
use futures::{sink::SinkExt, stream::StreamExt};

#[derive(Clone)]
struct ClusterState {
    nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
    front_transmission: Option<Sender<ServerLogType>>,
}

struct NodeInfo {
    address: String,
    port: String,
    process: Child,
}

fn app() -> Router {
    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");

    let state = ClusterState {
        nodes: Arc::new(RwLock::new(HashMap::new())),
        front_transmission: None,
    };

    Router::new()
        .fallback_service(ServeDir::new(assets_dir).append_index_html_on_directories(true))
        .route("/ws", any(ws_handler))
        .with_state(state)
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<ClusterState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, addr, state))
}

async fn handle_socket(socket: WebSocket, who: SocketAddr, mut state: ClusterState) {
    println!("socketing");
    let (mut socket_sender, mut socket_receiver) = socket.split();

    let (tx, mut rx) = mpsc::channel(100);

    state.front_transmission = Some(tx);
    let state2 = state.clone();

    let mut command_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = socket_receiver.next().await {
            if let Message::Text(text) = msg {
                match serde_json::from_str::<FrontendCommand>(&text) {
                    Ok(cmd) => match cmd {
                        FrontendCommand::AddNode { addr, port } => {
                            spawn_node(port, addr, &state).await;
                        }
                        FrontendCommand::RemoveNode { id } => {
                            remove_node(id, &state).await;
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
                    println!("Error serializing message: {:?}", e);
                    continue;
                }
            };

            // Send the message to the WebSocket
            if let Err(err) = socket_sender.send(msg.into()).await {
                println!("Error sending message to socket: {:?}", err);
            }
        }
    });


    //TODO: delete once the tests finished
    for addr in 0..3{
        let port = (4000 + addr).to_string();
        spawn_node(port, "127.0.0.1".into(), &state2).await;
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
enum FrontendCommand {
    AddNode { addr: String, port: String },
    RemoveNode { id: String },
}

#[tokio::main]
async fn main() {
    // build our application with some routes
    let app = app();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}

#[derive(Serialize, Deserialize)]
struct ServerLogSpan {
    addr: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct ServerLog {
    timestamp: String,
    span: ServerLogSpan,
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
    Create(ServerLog),
    ReceiveAppendEntries(ServerLog),
    ReceiveVote(ServerLog),
    BecomeCandidate(ServerLog),
    BecomeFollower(ServerLog),
    BecomeLeader(ServerLog),
}

async fn spawn_node(port: String, address: String, state: &ClusterState) {
    let full_addr = format!("{}:{}", address, port);
    println!("new node: {}", &full_addr);
    let mut cmd = Command::new("../target/debug/raft")
        .arg("--addr")
        .arg(full_addr.clone())
        .arg("--nodes")
        .arg("127.0.0.1:4000")
        .arg("127.0.0.1:4001")
        .arg("127.0.0.1:4002")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = cmd.stdout.take().unwrap();
    let rx = state.front_transmission.clone().unwrap();
    tokio::spawn(async move {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            match serde_json::from_str::<ServerLogType>(&line) {
                Ok(cmd) => {
                    if let Err(err) = rx.send(cmd).await {
                        println!("Error sending message to frontend");
                    }
                }
                Err(e) => {
                    println!("Error parsing log: {:?} ; {:?}", e, line);
                }
            }
        }
    });

    state.nodes.write().await.insert(
        full_addr,
        NodeInfo {
            port,
            address,
            process: cmd,
        },
    );
}

// Remove a node
async fn remove_node(id: String, state: &ClusterState) {
    let mut nodes = state.nodes.write().await;
    if let Some(mut node) = nodes.remove(&id) {
        node.process.kill().await.unwrap();
    }
}
