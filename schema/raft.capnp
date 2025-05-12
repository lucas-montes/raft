@0x9663f4dd604afa36;

interface Raft {

  appendEntries @0 (request: AppendEntriesRequest) -> (response: AppendEntriesResponse);

  requestVote @1 (term :UInt64, candidateId :Text, lastLogIndex :UInt64, lastLogTerm :UInt64) -> (voteGranted :Bool, term :UInt64);
}


struct AppendEntriesRequest {
  term @0 :UInt64;
  leaderId @1 :Text;
  prevLogIndex @2:UInt64;
  prevLogTerm @3 :UInt64;
  entries @4 :List(LogEntry);
  leaderCommit @5 :UInt64;
}

struct AppendEntriesResponse {
  term @0 :UInt64;
  success @1 :Bool;
}

struct LogEntry {
  index @0 :UInt64;
  term @1:UInt64;
  command @2 :Text;
}
