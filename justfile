default: run_ci

set export

run_ci: lint build test

@tokio target:
  echo setting executor to tokio
  export RUSTFLAGS='--cfg async_executor_impl="tokio" --cfg async_channel_impl="tokio" ${RUSTFLAGS:}'

@async_std target *ARGS:
  echo setting executor to async-std
  export RUSTFLAGS='--cfg async_executor_impl="async-std" --cfg async_channel_impl="async-std"' && just {{target}} {{ARGS}}

build:
  cargo build --verbose --profile=release-lto --workspace --examples --bins --tests --lib --benches

test:
  echo Testing
  cargo test --verbose --profile=release-lto --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

test_basic:
  echo Running the basic tests, including the test for success, the test for nodes leaving the network, and unit tests for network, consensus and DA tasks
  ASYNC_STD_THREAD_COUNT=1 cargo test  --lib --bins --tests --benches --workspace --no-fail-fast test_success test_with_failures test_network test_consensus test_da -- --test-threads=1 --nocapture

test_success:
  echo Testing success test
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_success -- --test-threads=1 --nocapture

test_web_server:
  echo Testing web server
  cargo test  --lib --bins --tests --benches --workspace --no-fail-fast web_server_network -- --test-threads=1 --nocapture

test_with_failures:
  echo Testing nodes leaving the network with async std executor
  ASYNC_STD_THREAD_COUNT=1 cargo test  --lib --bins --tests --benches --workspace --no-fail-fast test_with_failures -- --test-threads=1 --nocapture

test_network_task:
  echo Testing the DA task with async std executor
  ASYNC_STD_THREAD_COUNT=1 cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_network_task -- --test-threads=1 --nocapture

test_consensus_task:
  echo Testing with async std executor
  cargo test  --lib --bins --tests --benches --workspace --no-fail-fast test_consensus -- --test-threads=1 --nocapture

test_da_task:
  echo Testing the DA task with async std executor
  ASYNC_STD_THREAD_COUNT=1 cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_da_task -- --test-threads=1 --nocapture

test_pkg := "hotshot"

default_test := ""

test_name := "sequencing_libp2p_test"

run_test test=default_test:
  cargo test --verbose --release --lib --bins --tests --benches {{test}} --no-fail-fast -- --test-threads=1 --nocapture

test_pkg_all pkg=test_pkg:
  cargo test --verbose --release --lib --bins --tests --benches --package={{pkg}} --no-fail-fast -- --test-threads=1 --nocapture

list_tests_json package=test_pkg:
  RUST_LOG=none cargo test --verbose --profile=release-lto --lib --bins --tests --benches --package={{package}} --no-fail-fast -- --test-threads=1 -Zunstable-options --format json

list_examples package=test_pkg:
  cargo metadata | jq '.packages[] | select(.name == "{{package}}") | .targets[] | select(.kind  == ["example"] ) | .name'

check:
  echo Checking
  cargo check --workspace --bins --tests --examples

lint: fmt
  echo linting
  cargo clippy --workspace --bins --tests --examples -- -D warnings

fmt:
  echo Running cargo fmt
  cargo fmt

careful:
  echo Careful-ing with tokio executor
  cargo careful test --verbose --profile careful --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

fix:
  cargo fix --allow-dirty --allow-staged --workspace --lib --bins --tests --benches

doc:
  echo Generating docs
  cargo doc --no-deps --workspace --profile=release-lto --document-private-items --bins --examples --lib

doc_test:
  echo Test docs
  cargo test --doc --workspace
