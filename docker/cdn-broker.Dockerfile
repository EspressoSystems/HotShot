FROM ubuntu:jammy

ARG TARGETARCH
ARG ASYNC_EXECUTOR

COPY ./target/${ASYNC_EXECUTOR}/${TARGETARCH}/release/examples/cdn-broker /usr/local/bin/cdn-broker

# logging
ENV RUST_LOG="warn"

# log format. JSON no ansi
ENV RUST_LOG_FORMAT="json"

ENTRYPOINT ["cdn-broker"]
