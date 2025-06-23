fn main() {
    ::capnpc::CompilerCommand::new()
        .src_prefix("../schema")
        .file("../schema/storage.capnp")
        .run()
        .expect("compiling schema");
}
