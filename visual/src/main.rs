use askama::Template;
use axum::{
    body::Bytes,
    extract::{
        ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{any, get},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    io::AsyncBufReadExt,
    process::{Child, Command},
    sync::RwLock,
};

use std::{collections::HashMap, ops::ControlFlow, process::Stdio, sync::Arc};
use std::{net::SocketAddr, path::PathBuf};
use tower_http::services::ServeDir;

//allows to extract the IP of connecting user
use axum::extract::connect_info::ConnectInfo;
use axum::extract::ws::CloseFrame;

//allows to split the websocket stream into separate TX and RX branches
use futures::{sink::SinkExt, stream::StreamExt};

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

fn app() -> Router {
    let assets_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("templates");
    let state = ClusterState {
        nodes: Arc::new(RwLock::new(HashMap::new())),
        // telemetry: Arc::new(RwLock::new(HashMap::new())),
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

async fn handle_socket(socket: WebSocket, who: SocketAddr, state: ClusterState) {
    println!("socketing");
    let (mut sender, mut receiver) = socket.split();

    // Telemetry streaming task
    let telemetry_state = state.clone();
    let mut telemetry_task = tokio::spawn(async move {
        loop {
            // let telemetry = telemetry_state.telemetry.read().await;
            // for (node_id, telem) in telemetry.iter() {
            //     let msg = TelemetryMessage {
            //         event: TelemetryEvent::StateChange {
            //             node_id: node_id.clone(),
            //             role: telem.role.clone(),
            //             term: telem.term,
            //             commit_index: telem.commit_index,
            //         },
            //     };
            //     if sender.send(Message::Text(serde_json::to_string(&msg).unwrap().into())).await.is_err() {
            //         return;
            //     }
            // }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });

    // Command processing task
    let command_state = state.clone();
    let mut command_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                println!("{:?}", &text);
                match serde_json::from_str::<FrontendCommand>(&text) {
                    Ok(cmd) => {
                        process_command(cmd, who, &command_state).await;
                    }
                    Err(e) => {
                        println!("Error parsing command: {:?}", e);
                    }
                }
            }
        }
    });

    // Abort tasks if either exits
    tokio::select! {
        _ = (&mut telemetry_task) => command_task.abort(),
        _ = (&mut command_task) => telemetry_task.abort(),
    }
}

async fn process_command(cmd: FrontendCommand, who: SocketAddr, state: &ClusterState) {
    match cmd {
        FrontendCommand::GetState => {
            let nodes = state.nodes.read().await;
            // let telemetry = state.telemetry.read().await;
            // let msg = TelemetryMessage {
            //     event: TelemetryEvent::StateChange {
            //         node_id: "".to_string(),
            //         role: NodeRole::Leader, // Placeholder
            //         term: 0,
            //         commit_index: 0,
            //     },
            // };
            // Send state via WebSocket
        }
        FrontendCommand::AddNode { addr, port } => {
            spawn_node(port, addr, state).await;
        }
        FrontendCommand::RemoveNode { id } => {
            remove_node(id, state).await;
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
enum FrontendCommand {
    GetState,
    AddNode { addr: String, port: String },
    RemoveNode { id: String },
}

#[derive(Clone)]
struct ClusterState {
    nodes: Arc<RwLock<HashMap<String, NodeInfo>>>,
    // telemetry: Arc<RwLock<HashMap<String, NodeTelemetry>>>,
}

struct NodeInfo {
    address: String,
    port: String,
    process: Child,
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
    let a = full_addr.clone();
    tokio::spawn(async move {
        let reader = tokio::io::BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            println!("raft node {}: {}", a, line);
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
