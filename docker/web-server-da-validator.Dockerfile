FROM ubuntu:jammy

RUN apt-get update \
    &&  apt-get install -y curl libcurl4 wait-for-it tini \
    &&  rm -rf /var/lib/apt/lists/*

ARG TARGETARCH

COPY ./target/$TARGETARCH/debug/examples/web-server-da-validator /usr/local/bin/web-server-da-validator

# logging
ENV RUST_LOG="warn"

# log format. JSON no ansi
ENV RUST_LOG_FORMAT="json"

ENTRYPOINT ["tini", "--"]
CMD ["web-server-da-validator"]
