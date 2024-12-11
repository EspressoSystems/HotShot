FROM alpine:3

# Copy the source files
COPY ./target/release/examples/single-validator /single-validator

# Run the broker
ENTRYPOINT ["/single-validator"]
