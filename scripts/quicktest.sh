#!/bin/bash

# Runs tests like the CI would. Useful for PRs that don't trigger the CI

set -ex

cargo audit --deny warnings
cargo check
cargo build --verbose --workspace --all-targets --all-features --release
ulimit -n 4096
cargo test --verbose --release --lib --bins --tests --benches --all-features --workspace --no-fail-fast -- --test-threads=1
ulimit -n 4096
cargo test --verbose --release --workspace --all-features --no-fail-fast -- test_stress --test-threads=1 --ignored
