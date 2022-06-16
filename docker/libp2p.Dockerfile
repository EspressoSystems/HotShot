# NOTE run from parent directory
FROM ubuntu:jammy

# assuming this is built already
# to build:
#    `nix develop .#staticShell -c bash -c "cargo build --release  --examples --no-default-features --features=webui"`
COPY target/x86_64-unknown-linux-musl/release/examples/counter /bin/counter

# TODO these need to be overridden !
ENV BOOTSTRAP_LIST="18.224.1.60:9000,18.224.1.60:9000"
ENV NODE_TYPE="Regular"
ENV NUM_NODES="100"

CMD ["./counter --bound_addr 0.0.0.0:9000 --node_type $NODE_TYPE --num_nodes $NUM_NODES --bootstrap $BOOTSTRAP_LIST"]

