# HotShot Consensus Module

HotShot is a BFT consensus protocol based off of HotStuff, with the addition of proof-of-stake and
VRF committee elections.

# Usage

Please see the rustdoc for API documentation, and the examples directory for usage.

# Static linking

HotShot supports static linking for its examples:

```
# Nix-shell is optional but recommended
nix develop .#staticShell

cargo build --examples --all-features --all-targets --release --workspace
```

# Testing

To test:

```
RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT cargo test --verbose --release --lib --bins --tests --benches --all-features --workspace -- --nocapture --test-threads=1
```

- `RUST_LOG=$ERROR_LOG_LEVEL`: The basic levels of logging include `warn`, `error`, `info`.
- `RUST_LOG_FORMAT=$ERROR_LOG_FORMAT`: The types of logging include `full`, `json`, and `compact`.
- Inclusion of the `--nocapture` flag indicates whether or not to output logs.
- We run at `--test-threads=1` because the tests spawn up a lot of file handles, and unix based systems consistently run out of handles.

To stress test, run the ignored tests prefixed with `test_stress`:
```
RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT cargo test --verbose --release --lib --bins --tests --benches --all-features --workspace test_stress -- --nocapture --test-threads=1 --ignored
```

## Testing on CI

To test as if running on CI, one must limit the number of cores and ram to match github runners (2 core, 7 gig ram). To limit the ram, spin up a virtual machine or container with 7 gigs ram. To limit the core count when running tests:

```
ASYNC_STD_THREAD_COUNT=1 RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT cargo test --verbose --release --lib --bins --tests --benches --all-features --workspace -- --nocapture --test-threads=1
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
cargo test --verbose --release --lib --bins --tests --benches --all-features --workspace -- --test-threads=1
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
cargo-llvm-cov llvm-cov --test=test_stress_dht_many_round --workspace --all-targets --all-features --release --html --output-path lcov.html
```

This will output:
- `heaptrack.counter-$HASH` which is viewable by heaptrack. This provides a plethora of useful statistics about memory and cpu cycles.
- `flamegraph.svg` which is a (moderately) less detailed version of heaptrack.
- `lcov.html` generates a summary of code coverage.

# Git Workflow

For espresso developers we have written up a description of our workflow [here](./WORKFLOW.md).
