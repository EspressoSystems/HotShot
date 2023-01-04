# HotShot Consensus Module

HotShot is a BFT consensus protocol based off of HotStuff, with the addition of proof-of-stake and
VRF committee elections.

## Disclaimer

**DISCLAIMER:** This software is provided "as is" and its security has not been externally audited. Use at your own risk.

# Usage

Please see the rustdoc for API documentation, and the examples directory for usage.

## Dependencies

### Unix-like

#### Nix (macos and linux)

```
nix develop
```

#### Brew (macos)

```
brew install cmake protobuf
```

#### Apt-get (linux)

```
apt-get install cmake protobuf
```

### Windows

#### Chocolatey

```
choco install cmake protoc
```

#### Scoop

```
scoop bucket add extras
scoop install protobuf cmake
```

## Building

Once dependencies have been installed, to build everything:

```
cargo build --examples  --bins --tests --features=full-ci --all-targets --release --workspace
```



# Static linking

HotShot supports static linking for its examples:

```
# Nix-shell is optional but recommended
nix develop .#staticShell

cargo build --examples --features=full-ci --all-targets --release --workspace
```

# Testing

To test:

```
RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT cargo test --verbose --release --lib --bins --tests --benches --features=full-ci --workspace -- --nocapture --test-threads=1
```

- `RUST_LOG=$ERROR_LOG_LEVEL`: The basic levels of logging include `warn`, `error`, `info`.
- `RUST_LOG_FORMAT=$ERROR_LOG_FORMAT`: The types of logging include `full`, `json`, and `compact`.
- Inclusion of the `--nocapture` flag indicates whether or not to output logs.
- We run at `--test-threads=1` because the tests spawn up a lot of file handles, and unix based systems consistently run out of handles.

To stress test, run the ignored tests prefixed with `test_stress`:
```
RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT cargo test --verbose --release --lib --bins --tests --benches --features=full-ci --workspace test_stress -- --nocapture --test-threads=1 --ignored
```

## Careful

To double check for UB:

```bash
nix develop .#correctnessShell
just careful
```

## Testing on CI

To test as if running on CI, one must limit the number of cores and ram to match github runners (2 core, 7 gig ram). To limit the ram, spin up a virtual machine or container with 7 gigs ram. To limit the core count when running tests:

```
ASYNC_STD_THREAD_COUNT=1 RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT cargo test --verbose --release --lib --bins --tests --benches --features=full-ci --workspace -- --nocapture --test-threads=1
```

# Tokio-console

To use tokio-console, drop into the console shell:

```
nix develop .#consoleShell
```

Then, run an example. For example:

```
BOOTSTRAP_ADDRS="0.us-east-2.cluster.aws.espresso.network:9000,1.us-east-2.cluster.aws.espresso.network:9000,2.us-east-2.cluster.aws.espresso.network:9000,3.us-east-2.cluster.aws.espresso.network:9000,4.us-east-2.cluster.aws.espresso.network:9000,5.us-east-2.cluster.aws.espresso.network:9000,6.us-east-2.cluster.aws.espresso.network:9000" START_TIMESTAMP="2022-09-09 15:42:00 -04:00:00" BOOTSTRAP_MESH_N_HIGH=50 BOOTSTRAP_MESH_N_LOW=20 BOOTSTRAP_MESH_OUTBOUND_MIN=15 BOOTSTRAP_MESH_N=30 MESH_N_HIGH=50 MESH_N_LOW=20 MESH_OUTBOUND_MIN=15 MESH_N=30 NEXT_VIEW_TIMEOUT=10 PROPOSE_MIN_ROUND_TIME=1 PROPOSE_MAX_ROUND_TIME=10 cargo run  --release --features=tokio-executor,demo --no-default-features --example multi-machine-libp2p -- --num_nodes=7   --num_txn_per_round=200  --online_time=20 --bound_addr=0.0.0.0:9000 --seed=1234   --node_idx=6 --padding 30 --num_bootstrap 30
````

On a separate terminal, also drop into the console shell and start tokio-console:
```
nix develop .#consoleShell -c tokio-console
```

This second window should now display task usage.

# Open Telemetry + Jaeger Integration

To view distributed logs with just the centralized server and one client, first edit the `centralized_server/orchestrator` file to include have a threshold and num_nodes of 1.

Then open 3 terminals.

```bash
# Terminal 1
# Start the jaeger instance to view spans
docker run -d -p6831:6831/udp -p6832:6832/udp -p16686:16686 -p14268:14268 jaegertracing/all-in-one:latest

# Terminal 2
# Start the CDN
pushd centralized_server/orchestrator
nix develop .#consoleShell
export TOKIO_CONSOLE_ENABLED=false
cargo run  --release  --features=tokio-ci,profiling -- centralized 127.0.0.1 4748

# Terminal 3
# Start the client
cargo run --example=multi-machine-centralized --release  --features=tokio-ci,profiling -- 127.0.0.1 4748
```

# Resource Usage Statistics

To generate usage stats:
- build the test suite
- find the executable containing the test of interest
- run profiling tools

The executable `cargo` uses is shown in the output of `cargo test`.

For example, to profile `test_stress_dht_many_round`:

```bash
# bring profiling tooling like flamegraph and heaptrack into scope
nix develop .#perfShell

# show the executable we need run
# and build all test executables (required for subsequent steps)
cargo test --verbose --release --lib --bins --tests --benches --features=full-ci --workspace -- --test-threads=1
# the output cargo test contains the tests path:
#       Running `/home/jrestivo/work/crosscross/target/release/deps/counter-880b1ff53ee21dea test_stress --test-threads=1 --ignored`
#       running 7 tests
#       test test_stress_dht_many_rounds ... ok
#       ...

# a more detailed alternative to flamegraph
# NOTE: only works on linux
heaptrack $(fd -I "counter*" -t x | rg release) --ignored -- test_stress_dht_many_round --nocapture
# palette provides memory statistics, omission will provide cpu cycle stats as colors
# NOTE: must be run as root on macos
flamegraph --palette=mem $(fd -I "counter*" -t x | rg release) --ignored -- test_stress_dht_one_round
# code coveragte statistics
cargo-llvm-cov llvm-cov --test=test_stress_dht_many_round --workspace --all-targets --features=full-ci --release --html --output-path lcov.html
```

This will output:
- `heaptrack.counter-$HASH` which is viewable by heaptrack. This provides a plethora of useful statistics about memory and cpu cycles.
- `flamegraph.svg` which is a (moderately) less detailed version of heaptrack.
- `lcov.html` generates a summary of code coverage.

# Debugging

A debugging config file is provided for vscode and vscodium in [`.vscode/launch.json`](https://github.com/EspressoSystems/HotShot/blob/main/.cargo/config). This is intended to be used with [vadimcn/vscode-lldb](https://open-vsx.org/extension/vadimcn/vscode-lldb) but may work with other rust debuggers as well.

To bring `lldb` into scope with nix, run `nix develop .#debugShell`.

# Git Workflow

For espresso developers we have written up a description of our workflow [here](./WORKFLOW.md).

# Extra Editor Configuration

## Neovim

Copy `.nvim/example.env.lua` to `.nvim/env.lua`. The purpose of this file is to return to neovim the requisite feature flags.

Then load this file when configuring `rust-tools`:

```lua
rust_tools.setup(
  {server =
    {settings =
      {
        ["rust-analyzer"] =
          {cargo =
            {features =
              loadfile(".nvim/env.lua") and { loadfile(".nvim/env.lua")() } or "all"
            }
          }
      }
    }
  }
)
```

# License

## Copyright
`HotShot` was developed by Espresso Systems. While we plan to adopt an open source license, we have not yet selected one. As such, all rights are reserved for the time being. Please reach out to us if you have thoughts on licensing.  
