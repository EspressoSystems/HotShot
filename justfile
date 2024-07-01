default: run_ci

set export

original_rustflags := env_var_or_default('RUSTFLAGS', '--cfg hotshot_example')
original_rustdocflags := env_var_or_default('RUSTDOCFLAGS', '--cfg hotshot_example')
original_target_dir := env_var_or_default('CARGO_TARGET_DIR', 'target')

run_ci: lint build test

async := "async-std"

# Run arbitrary cargo commands, with e.g.
#     just async=async-std cargo check
# or
#     just async=tokio cargo test --tests test_consensus_task
# Defaults to async-std.

@cargo *ARGS:
  echo setting async executor to {{async}}
  export RUSTDOCFLAGS='-D warnings --cfg async_executor_impl="{{async}}" --cfg async_channel_impl="{{async}}" {{original_rustdocflags}}' RUSTFLAGS='--cfg async_executor_impl="{{async}}" --cfg async_channel_impl="{{async}}" {{original_rustflags}}' CARGO_TARGET_DIR='{{original_target_dir}}/{{async}}' && cargo {{ARGS}}

@tokio target *ARGS:
  echo setting executor to tokio
  export RUSTDOCFLAGS='-D warnings --cfg async_executor_impl="tokio" --cfg async_channel_impl="tokio" {{original_rustdocflags}}' RUSTFLAGS='--cfg async_executor_impl="tokio" --cfg async_channel_impl="tokio" {{original_rustflags}}' CARGO_TARGET_DIR='{{original_target_dir}}/tokio' && just {{target}} {{ARGS}}

@async_std target *ARGS:
  echo setting executor to async-std
  export RUST_MIN_STACK=4194304 RUSTDOCFLAGS='-D warnings --cfg async_executor_impl="async-std" --cfg async_channel_impl="async-std" {{original_rustdocflags}}' RUSTFLAGS='--cfg async_executor_impl="async-std" --cfg async_channel_impl="async-std" {{original_rustflags}}' CARGO_TARGET_DIR='{{original_target_dir}}/async-std' && just {{target}} {{ARGS}}

@async-std target *ARGS:
  echo setting executor to async-std
  export RUST_MIN_STACK=4194304 RUSTDOCFLAGS='-D warnings --cfg async_executor_impl="async-std" --cfg async_channel_impl="async-std" {{original_rustdocflags}}' RUSTFLAGS='--cfg async_executor_impl="async-std" --cfg async_channel_impl="async-std" {{original_rustflags}}' CARGO_TARGET_DIR='{{original_target_dir}}/async-std' && just {{target}} {{ARGS}}

build:
  cargo build --workspace --examples --bins --tests --lib --benches

build_release *ARGS:
  cargo build --profile=release {{ARGS}}

example *ARGS:
  cargo run --profile=release-lto --package hotshot-examples --no-default-features --example {{ARGS}}

example_fixed_leader *ARGS:
  cargo run --features "fixed-leader-election" --profile=release-lto --example {{ARGS}}

example_gpuvid_leader *ARGS:
  cargo run --features "fixed-leader-election, gpu-vid" --profile=release-lto --example {{ARGS}}

test *ARGS:
  echo Testing {{ARGS}}
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast {{ARGS}} -- --test-threads=1 --nocapture --skip crypto_test

test-ci *ARGS:
  echo Testing {{ARGS}}
  RUST_LOG=error,hotshot=debug,libp2p-networking=debug cargo test --lib --bins --tests --benches --workspace --no-fail-fast {{ARGS}} -- --test-threads=1

test-ci-rest *ARGS:
  echo Testing {{ARGS}}
  RUST_LOG=error,hotshot=debug,libp2p-networking=debug cargo test --lib --bins --tests --benches --workspace --no-fail-fast {{ARGS}} -- --skip tests_1 --skip tests_2 --skip tests_3 --skip tests_4 --skip tests_5 --test-threads=1

test-ci-1 *ARGS:
  echo Testing {{ARGS}}
  RUST_LOG=error,hotshot=debug,libp2p-networking=debug cargo test tests_1 --lib --bins --tests --benches --workspace --no-fail-fast {{ARGS}} -- --test-threads=1

test-ci-2 *ARGS:
  echo Testing {{ARGS}}
  RUST_LOG=error,hotshot=debug,libp2p-networking=debug cargo test tests_2 --lib --bins --tests --benches --workspace --no-fail-fast {{ARGS}} -- --test-threads=1

test-ci-3 *ARGS:
  echo Testing {{ARGS}}
  RUST_LOG=error,hotshot=debug,libp2p-networking=debug cargo test tests_3 --lib --bins --tests --benches --workspace --no-fail-fast {{ARGS}} -- --test-threads=1

test-ci-4 *ARGS:
  echo Testing {{ARGS}}
  RUST_LOG=error,hotshot=debug,libp2p-networking=debug cargo test tests_4 --lib --bins --tests --benches --workspace --no-fail-fast {{ARGS}} -- --test-threads=1

test-ci-5 *ARGS:
  echo Testing {{ARGS}}
  RUST_LOG=error,hotshot=debug,libp2p-networking=debug cargo test tests_5 --lib --bins --tests --benches --workspace --no-fail-fast {{ARGS}} -- --test-threads=1

test_basic: test_success test_with_failures test_network_task test_consensus_task test_da_task test_vid_task test_view_sync_task

test_catchup:
  echo Testing with async std executor
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_catchup -- --test-threads=1 --nocapture

test_crypto:
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast crypto_test -- --test-threads=1 --nocapture

test_success:
  echo Testing success test
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_success -- --test-threads=1 --nocapture

test_success_dependency_tasks:
  echo Testing success test
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast --features "dependency-tasks" test_success -- --test-threads=1 --nocapture

test_timeout:
  echo Testing timeout test
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_timeout -- --test-threads=1 --nocapture

test_combined_network:
  echo Testing combined network
  cargo test  --lib --bins --tests --benches --workspace --no-fail-fast test_combined_network -- --test-threads=1 --nocapture

test_web_server:
  echo Testing web server
  cargo test  --lib --bins --tests --benches --workspace --no-fail-fast web_server_network -- --test-threads=1 --nocapture

test_with_failures:
  echo Testing nodes leaving the network
  cargo test  --lib --bins --tests --benches --workspace --no-fail-fast test_with_failures -- --test-threads=1 --nocapture

test_with_failures_dependency_tasks:
  echo Testing nodes leaving the network with dependency tasks
  cargo test  --lib --bins --tests --benches --workspace --no-fail-fast --features "dependency-tasks" test_with_failures -- --test-threads=1 --nocapture

test_network_task:
  echo Testing the DA task
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_network_task -- --test-threads=1 --nocapture

test_memory_network:
  echo Testing the DA task
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast memory_network -- --test-threads=1 --nocapture

test_consensus_task:
  echo Testing the consensus task
  cargo test  --lib --bins --tests --benches --workspace --no-fail-fast test_consensus -- --test-threads=1 --nocapture

test_quorum_vote_task:
  echo Testing the quorum vote task
  cargo test  --lib --bins --tests --benches --workspace --no-fail-fast --features "dependency-tasks" test_quorum_vote_task -- --test-threads=1 --nocapture

test_quorum_proposal_task:
  echo Testing the quorum proposal task
  cargo test  --lib --bins --tests --benches --workspace --no-fail-fast --features "dependency-tasks" test_quorum_proposal_task -- --test-threads=1 --nocapture

test_da_task:
  echo Testing the DA task
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_da_task -- --test-threads=1 --nocapture

test_vid_task:
  echo Testing the VID task
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_vid_task -- --test-threads=1 --nocapture

test_view_sync_task:
  echo Testing the view sync task
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_view_sync_task -- --test-threads=1 --nocapture

test_quorum_proposal_recv_task:
  echo Testing the quorum proposal recv task
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast --features "dependency-tasks" test_quorum_proposal_recv_task -- --test-threads=1 --nocapture

test_upgrade_task:
  echo Testing the upgrade task without dependency tasks
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast test_upgrade_task -- --test-threads=1 --nocapture

test_upgrade_task_dependency_tasks:
  echo Testing the upgrade task without dependency tasks
  cargo test --lib --bins --tests --benches --workspace --no-fail-fast --features "dependency-tasks" test_upgrade_task -- --test-threads=1 --nocapture

test_pkg := "hotshot"

default_test := ""

test_name := "sequencing_libp2p_test"

run_test test=default_test:
  cargo test --lib --bins --tests --benches {{test}} --no-fail-fast -- --test-threads=1 --nocapture

test_pkg_all pkg=test_pkg:
  cargo test --lib --bins --tests --benches --package={{pkg}} --no-fail-fast -- --test-threads=1 --nocapture

list_tests_json package=test_pkg:
  RUST_LOG=none cargo test --lib --bins --tests --benches --package={{package}} --no-fail-fast -- --test-threads=1 -Zunstable-options --format json

list_examples package=test_pkg:
  cargo metadata | jq '.packages[] | select(.name == "{{package}}") | .targets[] | select(.kind  == ["example"] ) | .name'

check:
  echo Checking
  cargo check --workspace --bins --tests --examples

clippy:
  echo clippy
  cargo clippy --workspace --examples --bins --tests -- -D warnings

lint:
  echo linting
  cargo fmt --check
  cargo clippy --workspace --examples --bins --tests -- -D warnings

lint_release:
  echo linting
  cargo fmt --check
  cargo clippy --package hotshot --no-default-features --features="docs, doc-images" -- -D warnings

fmt:
  echo Running cargo fmt
  cargo fmt

fmt_lint:
  echo Formatting and linting
  cargo fmt
  cargo clippy --workspace --examples --bins --tests -- -D warnings

fmt_lint_dependency_tasks:
  echo Formatting and linting
  cargo fmt
  cargo clippy --workspace --examples --bins --tests --features "dependency-tasks" -- -D warnings

careful:
  echo Careful-ing with tokio executor
  cargo careful test --profile careful --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

semver *ARGS:
  #!/usr/bin/env bash
  echo Running cargo-semver-checks
  while IFS= read -r crate; do
    cargo semver-checks \
      --package "${crate}" {{ARGS}} || true;
  done < <(cargo workspaces list)

fix:
  cargo fix --allow-dirty --allow-staged --workspace --lib --bins --tests --benches

doc:
  echo Generating docs {{env_var('RUSTFLAGS')}}
  cargo doc --no-deps --bins --examples --lib -p 'hotshot-types'
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
  cargo test --features "randomized-leader-election" --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture --skip crypto_test

code_coverage:
  echo "Running code coverage"
  cargo-llvm-cov llvm-cov --lib --bins --tests --benches --release --workspace --lcov --output-path lcov.info -- --test-threads=1
