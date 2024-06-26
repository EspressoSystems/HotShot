FROM ubuntu:jammy

RUN apt-get update \
    &&  apt-get install -y curl libcurl4 wait-for-it tini \
    &&  rm -rf /var/lib/apt/lists/*

ARG ASYNC_EXECUTOR=async-std

COPY --chmod=0755 ./target/${ASYNC_EXECUTOR}/release-lto/examples/validator-push-cdn /usr/local/bin/validator-push-cdn

# logging
ENV RUST_LOG="warn"

# log format. JSON no ansi
ENV RUST_LOG_FORMAT="json"

ENTRYPOINT ["validator-push-cdn"]
