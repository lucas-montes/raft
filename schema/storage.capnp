@0x80f216b08d0ebb65;

struct Entry {
id @0: Text;
data @1: Data;
}


interface Command {
  startTransaction @0 () -> (leader: Command);

  create @1 (data :Data) -> (item :Item);

  get @2 (id :Text) -> (item :Item);

  query @3 () -> (query :Query);

    interface Item {
      read @0 () -> (data :Entry);
      update @1 (data :Data) -> (item :Item);
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
}
