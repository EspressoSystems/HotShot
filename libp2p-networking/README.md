# USAGE

Networking library inteded for use with Phaselock. Builds upon abstractions on libp2p-rs.

## CLI Demo

To get very verbose logging:

```bash
RUST_LOG_OUTPUT=OUTFILE RUST_LOG="trace" cargo run --release
```

The idea here is to spin up several nodes in a p2p network. These nodes can share messages with each other.

```
nix develop -c "RUST_LOG_OUTPUT=OUTFILE_0 RUST_LOG=error cargo run --release --example clichat -- -p 1111"
nix develop -c "RUST_LOG_OUTPUT=OUTFILE_0 RUST_LOG=error cargo run --release --example clichat -- /ip4/127.0.0.1/tcp/1111 -p 2222"
nix develop -c "RUST_LOG_OUTPUT=OUTFILE_0 RUST_LOG=error cargo run --release --example clichat -- /ip4/127.0.0.1/tcp/2222 -p 3333"
nix develop -c "RUST_LOG_OUTPUT=OUTFILE_0 RUST_LOG=error cargo run --release --example clichat -- /ip4/127.0.0.1/tcp/3333 -p 4444"
nix develop -c "RUST_LOG_OUTPUT=OUTFILE_0 RUST_LOG=error cargo run --release --example clichat -- /ip4/127.0.0.1/tcp/4444 -p 5555"
nix develop -c "RUST_LOG_OUTPUT=OUTFILE_0 RUST_LOG=error cargo run --release --example clichat -- /ip4/127.0.0.1/tcp/5555 -p 6666"
nix develop -c "RUST_LOG_OUTPUT=OUTFILE_0 RUST_LOG=error cargo run --release --example clichat -- /ip4/127.0.0.1/tcp/6666 -p 7777"
nix develop -c "RUST_LOG_OUTPUT=OUTFILE_0 RUST_LOG=error cargo run --release --example clichat -- /ip4/127.0.0.1/tcp/7777 -p 8888"
nix develop -c "RUST_LOG_OUTPUT=OUTFILE_0 RUST_LOG=error cargo run --release --example clichat -- /ip4/127.0.0.1/tcp/8888 -p 9999"
```

At this point the idea is that each node will continue to attempt to connect to nodes
until it hits at least 5 peers.

Use `Tab` to switch between messages and prompt. Press `Enter` to broadcast a message to all connected nodes.
Press `Right Arrow` to direct-send a message to a randomly selected peer.
Press `q` to quit the program from the messages view.

## Counter Single Machine Tests

Each node has its own counter. The idea behind these tests is to support "broadcast" messages and "direct" messages to increment each nodes counter.

`cargo test --release stress`

spawns off five integration tests.

- Two that uses gossipsub to broadcast a counter increment from one node to all other nodes
- Two where one node increments its counter, then direct messages all nodes to increment their counters
- One that intersperses both broadcast and increments.

This often times fails on MacOS due to ["too many open files."](https://github.com/EspressoSystems/phaselock-networking-demo/issues/18) The fix here is:

```bash
ulimit -n 10240
```

## Counter Multi-machine tests

In these tests, there are three types of nodes. `Regular` nodes that limit the number of incoming connections, `Bootstrap` nodes that allow all connections, and `Conductor` nodes that all nodes (bootstrap and regular) connect to and periodically ping with their state. This "conductor" node instructs nodes in the swarm to increment their state either via broadcast or direct messages in the same fashion as the single machine tests.

In the direct message case, the conductor will increment the state of a randomly chosen node, `i`. Then the conductor will direct message all other nodes to request node `i`'s counter and increment their counter to the value in `i`'s node. In the broadcast case, the conductor will increment the state of a randomly chose node, `i`, and tell `i` to broadcast this incremented state.

In both cases, the test terminates as successful when the conductor receives the incremented state from all other nodes. Then, the conductor sends a special "kill" message to all known nodes and waits for them to disconnect.

Metadata about the toplogy is currently read from an `identity_mapping.json` file that manually labels the type of node (bootstrap, regular, conductor). The conductor uses this to figure out information about all nodes in the network. The regular nodes use this to learn about their ip address and the addresses necessary to bootstrap onto the network. The boostrap nodes only use this to learn about their ip addresses.

### Running counter multi-machine tests

A sample invocation locally:

```bash
# run each line in a separate terminal
nix develop -c cargo run --features webui --release --example counter -- --bound_addr 127.0.0.1:9000 --node_type Bootstrap --num_nodes 5 --bootstrap 127.0.0.1:9000 --webui 127.0.0.1:8000
nix develop -c cargo run --features webui --release --example counter -- --bound_addr 127.0.0.1:9001 --node_type Regular --num_nodes 5 --bootstrap 127.0.0.1:9000 --webui 127.0.0.1:8001
nix develop -c cargo run --features webui --release --example counter -- --bound_addr 127.0.0.1:9002 --node_type Regular --num_nodes 5 --bootstrap 127.0.0.1:9000 --webui 127.0.0.1:8002
nix develop -c cargo run --features webui --release --example counter -- --bound_addr 127.0.0.1:9003 --node_type Regular --num_nodes 5 --bootstrap 127.0.0.1:9000 --webui 127.0.0.1:8003
nix develop -c cargo run --features webui --release --example counter -- --bound_addr 127.0.0.1:9004 --node_type Conductor --num_nodes 5 --bootstrap 127.0.0.1:9000 --webui 127.0.0.1:8004
```

To run on the AWS cluster, see [here](https://github.com/EspressoSystems/cloud-infrastructure/blob/c86873a5c647772836907fc206fce5702a5878bb/ansible/networking-demo/README.md).
