FROM ubuntu:jammy

RUN apt-get update \
    &&  apt-get install -y curl libcurl4 wait-for-it tini \
    &&  rm -rf /var/lib/apt/lists/*

ARG TARGETARCH

COPY ./target/$TARGETARCH/debug/examples/libp2p-validator /usr/local/bin/libp2p-validator

# logging
ENV RUST_LOG="warn"

# log format. JSON no ansi
ENV RUST_LOG_FORMAT="json"

ENTRYPOINT ["tini", "--"]
CMD ["libp2p-validator"]
