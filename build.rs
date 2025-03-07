fn main() {
    ::capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/raft.capnp")
        .run()
        .expect("compiling schema");
}
