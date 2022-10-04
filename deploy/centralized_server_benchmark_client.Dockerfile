# NOTE run from parent directory
FROM ubuntu:jammy

# assuming this is built already
COPY target/release/benchmark_client /bin/benchmark_client

# logging
ENV RUST_LOG="warn"

# log format. JSON no ansi
ENV RUST_LOG_FORMAT="json"

ENV HOST="0.us-east-2.cluster.aws.espresso.network"
ENV PORT="2345"

CMD ["sh", "-c", "RUST_LOG_FORMAT=${RUST_LOG} RUST_LOG_FORMAT=${RUST_LOG} /bin/benchmark_client ${HOST}:${PORT}"]
