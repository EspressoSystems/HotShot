FROM 279906117593.dkr.ecr.us-east-2.amazonaws.com/rust:2021-03-24 as builder
RUN mkdir /app
WORKDIR /app
COPY . /app
RUN cargo audit ---workspace || true
RUN cargo clippy --workspace || true
RUN cargo fmt --all -- --check || true
RUN cargo build --workspace --release || true
RUN cargo test --workspace --release || true

