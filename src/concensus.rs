use std::{cell::RefCell, fmt::Debug, net::SocketAddr, ops::Deref, rc::Rc, time::Duration};

use capnp::capability::Promise;
use capnp_rpc::{pry, rpc_twoparty_capnp, twoparty, RpcSystem};
use futures::AsyncReadExt;

use crate::raft_capnp::raft;

type Err = Box<dyn std::error::Error>;

#[derive(Debug, Default, Copy, Clone)]
pub enum Role {
    Leader{next_index: u64, match_index: u64},
    Candidate,
    #[default]
    Follower,
}

#[derive(Debug)]
struct LogEntry {
    term: u64,
    msg: String,
}

#[derive(Debug, Clone)]
pub struct Node(Rc<RefCell<State>>);

impl Node {
    pub fn role(&self)-> Role {
        self.0.borrow().role
    }

    pub fn addr(&self) -> SocketAddr {
        self.0.borrow().addr
    }

    pub fn current_term(&self) -> u64 {
        self.0.borrow().current_term
    }

    pub fn new(addr: SocketAddr) -> Self {

        Self (Rc::new(RefCell::new(State{
            current_term: 0,
            voted_for: None,
            log_entries: Vec::new(),
            commit_index: 0,
            last_applied: 0,
            role: Role::Follower,
            addr
        })))
}}

#[derive(Debug)]
struct State {
    current_term: u64,
    voted_for: Option<SocketAddr>,
    log_entries: Vec<LogEntry>,
    commit_index: u64,
    last_applied: u64,
    role: Role,
    addr: SocketAddr,
}



async fn send_req(client: &raft::Client, term: u64) -> Result<u64, capnp::Error> {
    let mut request = client.append_entries_request();
    request.get().set_term(term);

    let reply = request.send().promise.await?;
    let new_term = reply.get()?.get_term();

    println!(
        "from send_req, received term: {} success: {}",
        new_term,
        reply.get()?.get_success()
    );
    Ok(new_term)
}
