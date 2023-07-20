# NOTE run from parent directory
FROM ubuntu:jammy

# assuming this is built already
COPY target/release-lto/examples/web-server-da-validator /bin/web-server-da-validator

# the host to connect to. Must be an IP address
ENV HOST="0.0.0.0"

# the port to connect to
ENV PORT="2345"

# logging
ENV RUST_LOG="warn"

# log format. JSON no ansi
ENV RUST_LOG_FORMAT="json"

CMD ["sh", "-c", "/bin/web-server-da-validator $HOST $PORT"]
