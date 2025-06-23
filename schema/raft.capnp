@0x9663f4dd604afa36;


using Storage = import "storage.capnp";


interface Raft extends (Storage.Command) {
  appendEntries @0 (request: AppendEntriesRequest) -> (response: AppendEntriesResponse);
  requestVote @1 (term :UInt64, candidateId :Text, lastLogIndex :UInt64, lastLogTerm :UInt64) -> (response: VoteResponse);
  getLeader @2 () -> (leader: Raft);
  joinCluster @3 (peer: Peer, history: NodeHistory) -> (peers: List(Peer));
}


interface LogEntriesHandler {
  get @0 (lastLogIndex: UInt64, lastLogTerm: UInt64) -> (entries: List(LogEntry));
}

struct NodeHistory {
  lastLogIndex @0 :UInt64;
  lastLogTerm @1 :UInt64;
}

struct AppendEntriesRequest {
  term @0 :UInt64;
  leaderId @1 :Text;
  prevLogIndex @2:UInt64;
  prevLogTerm @3 :UInt64;
  entries @4 :List(LogEntry);
  leaderCommit @5 :UInt64;
  # handleEntries @6 : LogEntriesHandler;
}

struct AppendEntriesResponse {
union {
  err @0 :UInt64;
  ok @1 :Void;
  }
}

struct VoteResponse {
  term @0 :UInt64;
  voteGranted @1 :Bool;
}

struct LogEntry {
  index @0 :UInt64;
  term @1:UInt64;
  command @2 :Data;
}

struct Peer {
  id @0 :Text;
  address @1 :Text;
  # client @2 :Raft;
}

struct HardState {
  currentTerm @0 : UInt64;

  # votedFor is optional: either `none` or a text‐encoded NodeId ("ip:port")
  votedFor : union {
    none   @1 : Void;
    nodeId @2 : Text;
  }

  logEntries  @3 : List(LogEntry);
}
