default: run_ci

run_ci: lint build test

build: build_tokio build_async_std

build_tokio:
  echo Building with tokio executor
  cargo build --verbose --release --workspace --examples --bins --tests --lib --benches --features=tokio-ci

build_async_std:
  echo Building with async std executor
  cargo build --verbose --release --workspace --examples --bins --tests --lib --benches --features=full-ci

test: test_tokio test_async_std

test_tokio:
  echo Testing with tokio executor
  cargo test --verbose --release --features=tokio-ci --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

test_async_std:
  echo Testing with async std executor
  cargo test --verbose --release --features=full-ci --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

lint: fmt lint_tokio lint_tokio_flume lint_async_std lint_async_std_flume

fmt:
  echo Running cargo fmt
  cargo fmt -- --check

lint_tokio:
  echo Linting with tokio executor
  cargo clippy --workspace --all-targets --no-default-features --features=tokio-executor,demo,docs,doc-images,hotshot-testing,channel-tokio --bins --tests --examples -- -D warnings

lint_tokio_flume:
  echo Linting with tokio executor and flume
  cargo clippy --workspace --all-targets --no-default-features --features=tokio-executor,demo,docs,doc-images,hotshot-testing,channel-flume --bins --tests --examples -- -D warnings

lint_async_std:
  echo Linting with async std executor
  cargo clippy --workspace --all-targets --no-default-features --features=async-std-executor,demo,docs,doc-images,hotshot-testing,channel-async-std --bins --tests --examples -- -D warnings

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
