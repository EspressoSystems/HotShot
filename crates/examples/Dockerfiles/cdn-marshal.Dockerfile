FROM alpine:3

# Copy the source files
COPY ./target/release/examples/cdn-marshal /cdn-marshal

# Run the broker
ENTRYPOINT ["/cdn-marshal"]
