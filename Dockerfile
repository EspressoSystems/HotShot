FROM 279906117593.dkr.ecr.us-east-2.amazonaws.com/rust:2021-03-24 as builder
RUN mkdir /app
WORKDIR /app
COPY . /app
RUN cargo audit || true
RUN cargo clippy -- -D warnings --features=async-std-executor
RUN cargo fmt -- --check
RUN cargo build --release --features=async-std-executor
RUN cargo test --release --features=full-ci

