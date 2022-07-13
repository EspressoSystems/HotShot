# Runs tests like the CI would. Useful for PRs that don't trigger the CI

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$PSDefaultParameterValues['*:ErrorAction']='Stop'

!(cargo fmt --all) -and
!(cargo clippy --workspace --all-targets --all-features -- -D warnings) -and
!(cargo audit --deny warnings) -and
!(cargo check) -and
!(cargo build --workspace --all-targets --all-features --release) -and
!(cargo test --release --lib --bins --tests --benches --all-features --workspace --no-fail-fast -- --test-threads=1) -and
!(cargo test --release --workspace --all-features --no-fail-fast -- test_stress --test-threads=1 --ignored)
