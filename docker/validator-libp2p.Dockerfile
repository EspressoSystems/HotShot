FROM ubuntu:jammy

RUN apt-get update \
    &&  apt-get install -y curl libcurl4 wait-for-it tini \
    &&  rm -rf /var/lib/apt/lists/*

ARG TARGETARCH
ARG ASYNC_EXECUTOR

COPY --chmod=0755 ./target/${ASYNC_EXECUTOR}/${TARGETARCH}/release/examples/validator-libp2p /usr/local/bin/validator-libp2p

# logging
ENV RUST_LOG="warn"

# log format. JSON no ansi
ENV RUST_LOG_FORMAT="json"

ENTRYPOINT ["validator-libp2p"]
