@0x9663f4dd604afa36;

struct Entry {
id @0: UInt64;
data @1: Data;
}


interface Command {
  startTransaction @0 () -> (leader: Command);

  create @1 (data :Data) -> (item :Item);

  get @2 (id :Text) -> (item :Item);

  query @3 () -> (query :Query);

}

interface Item {
  read @0 () -> (data :Entry);
  update @1 (data :Entry) -> (item :Item);
  delete @2 () -> (status :Bool);
}

interface Query {
  filter @0 (key :Text, value :Data) -> (query :Query);
  limit @1 (count :UInt32) -> (query :Query);
  execute @2 () -> (result :ResultSet);
}

interface ResultSet {
  next @0 () -> (item :Item);
  all @1 () -> (items :List(Entry));
}


interface Raft {
  appendEntries @0 (request: AppendEntriesRequest) -> (response: AppendEntriesResponse);
  requestVote @1 (term :UInt64, candidateId :Text, lastLogIndex :UInt64, lastLogTerm :UInt64) -> (response: VoteResponse);

}


  interface LogEntriesHandler {
    get @0 (lastLogIndex: UInt64, lastLogTerm: UInt64) -> (entries: List(LogEntry));
  }

struct AppendEntriesRequest {
  term @0 :UInt64;
  leader @1 :Peer;
  prevLogIndex @2:UInt64;
  prevLogTerm @3 :UInt64;
  entries @4 :List(LogEntry);
  leaderCommit @5 :UInt64;
  handleEntries @6 : LogEntriesHandler;
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
  command @2 :Text;
}

struct Peer {
  id @0 :Text;
  address @1 :Text;
  client @2 :Raft;
}
