FROM 279906117593.dkr.ecr.us-east-2.amazonaws.com/rust:2021-03-24 as builder
RUN mkdir /app
WORKDIR /app
COPY . /app
RUN cargo audit || true
RUN cargo clippy || true
RUN cargo fmt -- --check || true
RUN cargo build --release || true
RUN cargo test --release || true

