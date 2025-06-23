fn main() {
    ::capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/raft.capnp")
        .file("schema/storage.capnp")
        .run()
        .expect("compiling schema");
}
