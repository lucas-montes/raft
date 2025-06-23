# Raft
A simple implementation of the Raft consensus protocol.

The implementation is not fully complete — snapshot functionality is not implemented, storage-related requests (CRUD operations) are missing, and dynamic cluster configuration may be somewhat naive and slightly incorrect.
However, you can still test the core functionality.

## Launch It
If you have Nix installed, you can simply run:

```bash
run-raft
```
Then, open your browser and go to [localhost:3000](http://localhost:3000).

Otherwise, you'll need to build the main crate with:
```bash
cargo build
```
then navigate to the visual crate and run it with:
```bash
cargo run
```

## Crates
Main crate: Contains the core logic for the consensus algorithm, as well as server-side code, client-side logic, storage, and peer management.

Visual crate: Provides a visualization of how the cluster behaves. It uses Command to spawn child processes running the Raft code and reads their logs from stdout.

Client crate: A helper crate used to send requests to the cluster and perform CRUD operations.
