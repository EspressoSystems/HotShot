default: run_ci

set export

original_rustflags := env_var_or_default('RUSTFLAGS', '--cfg hotshot_example')
original_rustdocflags := env_var_or_default('RUSTDOCFLAGS', '--cfg hotshot_example')

run_ci: lint build test

configure_async_executor executor:
  @echo setting executor to {{executor}}
  @export RUSTDOCFLAGS='-D warnings --cfg async_executor_impl="{{executor}}" --cfg async_channel_impl="{{executor}}" {{original_rustdocflags}}'
  @export RUSTFLAGS='--cfg async_executor_impl="{{executor}}" --cfg async_channel_impl="{{executor}}" {{original_rustflags}}'

@with_tokio *ARGS: (configure_async_executor "tokio")
  {{ARGS}}

@with_async_std *ARGS: (configure_async_executor "async-std")
  export RUST_MIN_STACK=4194304
  {{ARGS}}

@tokio target *ARGS: (configure_async_executor "tokio")
  just {{target}} {{ARGS}}

@async_std target *ARGS: (configure_async_executor "async-std")
  export RUST_MIN_STACK=4194304
  just {{target}} {{ARGS}}

build:
  cargo build --workspace --examples --bins --tests --lib --benches

example *ARGS:
  cargo run --profile=release-lto --example {{ARGS}}

test:
  echo Testing
  cargo test --verbose --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture --skip crypto_test

test_basic: test_success test_with_failures test_network_task test_consensus_task test_da_task test_vid_task test_view_sync_task

test_catchup:
  echo Testing with async std executor
  ASYNC_STD_THREAD_COUNT=2 cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_catchup -- --test-threads=1 --nocapture

test_crypto:
  ASYNC_STD_THREAD_COUNT=2 cargo test --lib --bins --tests --benches --workspace --no-fail-fast crypto_test -- --test-threads=1 --nocapture

test_success:
  echo Testing success test
  ASYNC_STD_THREAD_COUNT=2 cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_success -- --test-threads=1 --nocapture

test_timeout:
  echo Testing timeout test
  ASYNC_STD_THREAD_COUNT=2 cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_timeout -- --test-threads=1 --nocapture

test_combined_network:
  echo Testing combined network
  ASYNC_STD_THREAD_COUNT=2 cargo test  --lib --bins --tests --benches --workspace --no-fail-fast test_combined_network -- --test-threads=1 --nocapture

test_web_server:
  echo Testing web server
  ASYNC_STD_THREAD_COUNT=2 cargo test  --lib --bins --tests --benches --workspace --no-fail-fast web_server_network -- --test-threads=1 --nocapture

test_with_failures:
  echo Testing nodes leaving the network with async std executor
  ASYNC_STD_THREAD_COUNT=2 cargo test  --lib --bins --tests --benches --workspace --no-fail-fast test_with_failures -- --test-threads=1 --nocapture

test_network_task:
  echo Testing the DA task with async std executor
  ASYNC_STD_THREAD_COUNT=2 cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_network_task -- --test-threads=1 --nocapture

test_memory_network:
  echo Testing the DA task with async std executor
  ASYNC_STD_THREAD_COUNT=2 cargo test --lib --bins --tests --benches --workspace --no-fail-fast memory_network -- --test-threads=1 --nocapture

test_consensus_task:
  echo Testing with async std executor
  ASYNC_STD_THREAD_COUNT=2 cargo test  --lib --bins --tests --benches --workspace --no-fail-fast test_consensus -- --test-threads=1 --nocapture

test_da_task:
  echo Testing the DA task with async std executor
  ASYNC_STD_THREAD_COUNT=2 cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_da_task -- --test-threads=1 --nocapture

test_vid_task:
  echo Testing the VID task with async std executor
  ASYNC_STD_THREAD_COUNT=2 cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_vid_task -- --test-threads=1 --nocapture

test_view_sync_task:
  echo Testing the view sync task with async std executor
  ASYNC_STD_THREAD_COUNT=2 cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_view_sync_task -- --test-threads=1 --nocapture

test_pkg := "hotshot"

default_test := ""

test_name := "sequencing_libp2p_test"

run_test test=default_test:
  cargo test --verbose --release --lib --bins --tests --benches {{test}} --no-fail-fast -- --test-threads=1 --nocapture

test_pkg_all pkg=test_pkg:
  cargo test --verbose --release --lib --bins --tests --benches --package={{pkg}} --no-fail-fast -- --test-threads=1 --nocapture

list_tests_json package=test_pkg:
  RUST_LOG=none cargo test --verbose --lib --bins --tests --benches --package={{package}} --no-fail-fast -- --test-threads=1 -Zunstable-options --format json

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
  echo Generating docs {{env_var('RUSTFLAGS')}}
  cargo doc --no-deps --workspace --document-private-items --bins --examples --lib

doc_test:
  echo Test docs
  cargo test --doc --workspace

lint_imports:
  echo Linting imports
  cargo fmt --all -- --config unstable_features=true,imports_granularity=Crate

gen_key_pair:
  echo Generating key pair from config file in config/
  cargo test --package hotshot-testing --test gen_key_pair -- tests --nocapture

test_randomized_leader_election:
  echo Testing
  cargo test --features "randomized-leader-election" --verbose --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture --skip crypto_test 
