FROM alpine:3

# Copy the source files
COPY ./target/release/examples/cdn-broker /cdn-broker

# Run the broker
ENTRYPOINT ["/cdn-broker"]
