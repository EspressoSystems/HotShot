default: run_ci

run_ci: lint build test

build: build_tokio build_async_std

build_tokio:
  echo Building with tokio executor
  cargo build --verbose --profile=release-lto --workspace --examples --bins --tests --lib --benches --features=tokio-ci

build_async_std:
  echo Building with async std executor
  cargo build --verbose --profile=release-lto --workspace --examples --bins --tests --lib --benches --features=full-ci

test: test_tokio test_async_std_all

test_tokio:
  echo Testing with tokio executor
  cargo test --verbose --profile=release-lto --features=tokio-ci --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

test_async_std_all:
  echo Testing with async std executor
  cargo test  --features=full-ci --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1

_test_basic:
  echo Testing with async std executor
  RUST_LOG="" cargo test  --features=full-ci --lib --bins --tests --benches --workspace --no-fail-fast test_basic -- --test-threads=1 --nocapture

_test_basic_tokio:
  echo Testing with async std executor
  RUST_LOG="" cargo test  --features=tokio-ci --lib --bins --tests --benches --workspace --no-fail-fast test_basic -- --test-threads=1 --nocapture

test_with_failures:
  echo Testing with async std executor
  RUST_LOG="" cargo test  --features=full-ci --lib --bins --tests --benches --workspace --no-fail-fast test_with_failures -- --test-threads=1 --nocapture

test_pkg := "hotshot"

test_name := "sequencing_libp2p_test"

test_async_std_pkg_all pkg=test_pkg:
  cargo test --verbose --release --features=async-std-executor,demo,channel-async-std --lib --bins --tests --benches --package={{pkg}} --no-fail-fast -- --test-threads=1 --nocapture


test_async_std_pkg_test name=test_name:
  cargo test --verbose --release --features=async-std-executor,demo,channel-async-std --lib --bins --tests --benches --workspace --no-fail-fast {{name}} -- --test-threads=1 --nocapture

list_tests_json package=test_pkg:
  RUST_LOG=none cargo test --verbose --profile=release-lto --features=full-ci,channel-async-std --lib --bins --tests --benches --package={{package}} --no-fail-fast -- --test-threads=1 -Zunstable-options --format json

list_examples package=test_pkg:
  cargo metadata | jq '.packages[] | select(.name == "{{package}}") | .targets[] | select(.kind  == ["example"] ) | .name'

check: check_tokio check_tokio_flume check_async_std check_async_std_flume

check_tokio:
  echo Checking with tokio executor
  cargo check --workspace --all-targets --no-default-features --features=tokio-executor,demo,docs,doc-images,hotshot-testing,channel-tokio --bins --tests --examples

check_tokio_flume:
  echo Checking with tokio executor and flume
  cargo check --workspace --all-targets --no-default-features --features=tokio-executor,demo,docs,doc-images,hotshot-testing,channel-flume --bins --tests --examples

check_async_std:
  echo Checking with async std executor
  cargo check --workspace --all-targets --no-default-features --features=async-std-executor,demo,docs,doc-images,hotshot-testing,channel-async-std --bins --tests --examples

check_async_std_flume:
  echo Checking with async std executor and flume
  cargo check --workspace --all-targets --no-default-features --features=async-std-executor,demo,docs,doc-images,hotshot-testing,channel-flume --bins --tests --examples


lint: fmt lint_tokio lint_tokio_flume lint_async_std lint_async_std_flume

fmt:
  echo Running cargo fmt
  cargo fmt

lint_tokio:
  echo Linting with tokio executor
  cargo clippy --workspace --all-targets --no-default-features --features=tokio-executor,demo,docs,doc-images,hotshot-testing,channel-tokio --bins --tests --examples -- -D warnings

lint_tokio_flume:
  echo Linting with tokio executor and flume
  cargo clippy --workspace --all-targets --no-default-features --features=tokio-executor,demo,docs,doc-images,hotshot-testing,channel-flume --bins --tests --examples -- -D warnings

lint_async_std:
  echo Linting with async std executor
  cargo clippy --workspace --all-targets --no-default-features --features=async-std-executor,demo,docs,doc-images,hotshot-testing,channel-async-std,slow-tests --bins --tests --examples

lint_async_std_flume:
  echo Linting with async std executor and flume
  cargo clippy --workspace --all-targets --no-default-features --features=async-std-executor,demo,docs,doc-images,hotshot-testing,channel-flume --bins --tests --examples -- -D warnings

careful: careful_tokio careful_async_std

careful_tokio:
  echo Careful-ing with tokio executor
  cargo careful test --verbose --profile careful --features=tokio-ci --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

careful_async_std:
  echo Careful-ing with async std executor
  cargo careful test --verbose --profile careful --features=full-ci --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

fix_async_std:
  cargo fix --allow-dirty --allow-staged --features=full-ci,channel-async-std --workspace --lib --bins --tests --benches

doc:
  echo Generating docs
  cargo doc --no-deps --workspace --profile=release-lto --document-private-items --bins --examples --features=full-ci --lib

doc_test:
  echo Test docs
  cargo test --doc --workspace --features=full-ci
