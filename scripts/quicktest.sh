#!/bin/bash

# Runs tests like the CI would. Useful for PRs that don't trigger the CI

set -ex

cargo fmt --all
cargo clippy --workspace --all-targets --features=full-ci -- -D warnings
cargo audit --deny warnings
cargo check
cargo build --verbose --workspace --all-targets --features=full-ci --release
ulimit -n 4096
cargo test --verbose --release --lib --bins --tests --benches ---features=full-ci --workspace --no-fail-fast -- --test-threads=1
ulimit -n 4096
cargo test --verbose --release --workspace --features=full-ci --no-fail-fast -- test_stress --test-threads=1 --ignored
