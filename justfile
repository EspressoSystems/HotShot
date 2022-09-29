default: run_ci

run_ci: lint build test

build: build_tokio build_async_std

build_tokio:
  echo Building with tokio executor
  cargo build --verbose --release --workspace --examples --bins --tests --lib --benches --features=full-ci

build_async_std:
  echo Building with async std executor
  cargo build --verbose --release --workspace --examples --bins --tests --lib --benches --features=tokio-ci

test: test_tokio test_async_std

test_tokio:
  echo Testing with tokio executor
  cargo test --verbose --release --features=full-ci --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

test_async_std:
  echo Testing with async std executor
  cargo test --verbose --release --features=full-ci --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

lint: fmt lint_tokio lint_async_std

fmt:
  echo Running cargo fmt
  cargo fmt -- --check

lint_tokio:
  echo Linting with tokio executor
  cargo clippy --workspace --all-targets --no-default-features --features=full-ci --bins --tests --examples -- -D warnings

lint_async_std:
  echo Linting with async std executor
  cargo clippy --workspace --all-targets --no-default-features --features=tokio-ci --bins --tests --examples -- -D warnings

careful: careful_tokio careful_async_std

careful_tokio:
  echo Careful-ing with tokio executor
  cargo careful test --verbose --profile careful --features=full-ci --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture

careful_async_std:
  echo Careful-ing with async std executor
  cargo careful test --verbose --profile careful --features=full-ci --lib --bins --tests --benches --workspace --no-fail-fast -- --test-threads=1 --nocapture
