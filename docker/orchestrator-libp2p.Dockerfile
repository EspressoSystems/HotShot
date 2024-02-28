FROM ubuntu:jammy

RUN apt-get update \
    &&  apt-get install -y curl libcurl4 wait-for-it tini \
    &&  rm -rf /var/lib/apt/lists/*

ARG TARGETARCH

COPY ./target/release-lto/examples/orchestrator-libp2p /usr/local/bin/orchestrator-libp2p

# logging
ENV RUST_LOG="warn"

# log format. JSON no ansi
ENV RUST_LOG_FORMAT="json"

ENTRYPOINT "orchestrator-libp2p"
