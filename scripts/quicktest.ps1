# Runs tests like the CI would. Useful for PRs that don't trigger the CI

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$PSDefaultParameterValues['*:ErrorAction']='Stop'

cargo audit --deny warnings
cargo check
cargo build --workspace --all-targets --all-features --release
cargo test --release --lib --bins --tests --benches --all-features --workspace --no-fail-fast -- --test-threads=1
cargo test --release --workspace --all-features --no-fail-fast -- test_stress --test-threads=1 --ignored
