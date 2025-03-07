@0x9663f4dd604afa35;

interface Raft {

  appendEntries @0 (term :UInt64, leaderId :Text, prevLogIndex :UInt64, prevLogTerm :UInt64, entries :List(LogEntry), leaderCommit :UInt64) -> (term :UInt64, success :Bool);

  requestVote @1 (term :UInt64, candidateId :Text, lastLogIndex :UInt64, lastLogTerm :UInt64) -> (voteGranted :Bool, term :UInt64);
}

struct LogEntry {
  term @0 :UInt64;
  command @1 :Text;
}
