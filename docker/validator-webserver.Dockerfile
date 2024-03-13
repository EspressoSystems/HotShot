FROM ubuntu:jammy

RUN apt-get update \
    &&  apt-get install -y curl libcurl4 wait-for-it tini \
    &&  rm -rf /var/lib/apt/lists/*

ARG TARGETARCH
ARG ASYNC_EXECUTOR

COPY ./target/${ASYNC_EXECUTOR}/${TARGETARCH}/release-lto/examples/validator-webserver /usr/local/bin/validator-webserver

# logging
ENV RUST_LOG="warn"

# log format. JSON no ansi
ENV RUST_LOG_FORMAT="json"

ENTRYPOINT ["tini", "--"]
CMD ["validator-webserver"]
