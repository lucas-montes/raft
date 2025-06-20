# Raft
A simple implementation of the Raft concensus protocol.
The implementation it's not completly finished, the snapshot functionality isn't implemented neither the storage related requests (CRUD operations) and the dynamic cluster configuration might be too naive and slightly incorrect.
Yet you can test the main functionality.

## Launch it
If you have nix installed you can simple run:
```bash
run-raft
```
Then go to:
`127.0.0.1:3000`
