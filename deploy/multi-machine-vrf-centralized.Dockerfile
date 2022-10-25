# NOTE run from parent directory
FROM ubuntu:jammy

# assuming this is built already
COPY target/release-lto/examples/multi-machine-vrf-centralized /bin/multi-machine-vrf-centralized

# the host to connect to. Must be an IP address
ENV HOST="0.0.0.0"

# the port to connect to
ENV PORT="2345"

# logging
ENV RUST_LOG="error"

# log format. JSON no ansi
ENV RUST_LOG_FORMAT="json"

CMD ["sh", "-c", "/bin/multi-machine-vrf-centralized $HOST $PORT"]