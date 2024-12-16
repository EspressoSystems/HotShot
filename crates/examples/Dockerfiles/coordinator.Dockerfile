FROM alpine:3

# Copy the source files
COPY ./target/release/examples/coordinator /coordinator

# Run the broker
ENTRYPOINT ["/coordinator"]
