default: run_ci

set export

run_ci: lint build test

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

test-ci-rest *ARGS:
  echo Running unit tests
  RUST_LOG=none cargo nextest run -E 'not (test(tests_1) | test(tests_2) | test(tests_3) | test(tests_4) | test(tests_5) | test(tests_6))' --profile ci --lib --bins --tests --benches --workspace --no-fail-fast {{ARGS}}

test-ci-1:
  echo Running integration test group 1
  RUST_LOG=none cargo nextest run --profile ci tests_1 --lib --bins --tests --benches --workspace --no-fail-fast

test-ci-2:
  echo Running integration test group 2
  RUST_LOG=none cargo nextest run --profile ci tests_2 --lib --bins --tests --benches --workspace --no-fail-fast

test-ci-3:
  echo Running integration test group 3
  RUST_LOG=none cargo nextest run --profile ci tests_3 --lib --bins --tests --benches --workspace --no-fail-fast

test-ci-4:
  echo Running integration test group 4
  RUST_LOG=none cargo nextest run --profile ci tests_4 --lib --bins --tests --benches --workspace --no-fail-fast

test-ci-5:
  echo Running integration test group 5
  RUST_LOG=none cargo nextest run --profile ci tests_5 --lib --bins --tests --benches --workspace --no-fail-fast

test-ci-6-1:
  echo Running integration test group 6
  RUST_LOG=none cargo nextest run --profile ci tests_6 --lib --bins --tests --benches --workspace --no-fail-fast --partition hash:1/6

test-ci-6-2:
  echo Running integration test group 6
  RUST_LOG=none cargo nextest run --profile ci tests_6 --lib --bins --tests --benches --workspace --no-fail-fast --partition hash:2/6

test-ci-6-3:
  echo Running integration test group 6
  RUST_LOG=none cargo nextest run --profile ci tests_6 --lib --bins --tests --benches --workspace --no-fail-fast --partition hash:3/6

test-ci-6-4:
  echo Running integration test group 6
  RUST_LOG=none cargo nextest run --profile ci tests_6 --lib --bins --tests --benches --workspace --no-fail-fast --partition hash:4/6

test-ci-6-5:
  echo Running integration test group 6
  RUST_LOG=none cargo nextest run --profile ci tests_6 --lib --bins --tests --benches --workspace --no-fail-fast --partition hash:5/6

test-ci-6-6:
  echo Running integration test group 6
  RUST_LOG=none cargo nextest run --profile ci tests_6 --lib --bins --tests --benches --workspace --no-fail-fast --partition hash:6/6

# Usage:
#
#   just test memoryimpl_::test_success
#
# To display logs from a test run:
#
#   just test memoryimpl_::test_success --nocapture
test *ARGS:
  echo Running test {{ARGS}}
  cargo nextest run --profile local {{ARGS}} --lib --bins --tests --benches --workspace

check:
  echo Checking
  cargo check --workspace --bins --tests --examples

clippy:
  echo clippy
  cargo clippy --workspace --examples --bins --tests -- -D warnings

clippy_release:
  echo clippy release
  cargo clippy --package hotshot --no-default-features --features="docs, doc-images" -- -D warnings

fmt:
  echo Running cargo fmt
  cargo fmt -- crates/**/*.rs
  cargo fmt -- crates/**/tests/**/**.rs

fmt_check:
  echo Running cargo fmt --check
  cargo fmt --check -- crates/**/*.rs
  cargo fmt --check -- crates/**/tests/**/**.rs

lint: clippy fmt_check

lint_release: clippy_release fmt_check

fmt_clippy: fmt clippy

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
  echo Generating docs
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
