# NOTE run from parent directory
FROM ubuntu:jammy

# assuming this is built already
COPY target/release/examples/multi-machine-libp2p /bin/multi-machine-libp2p

# total number of nodes: regular + bootstrap
ENV NUM_NODES="20"

# number of bootstrap
ENV NUM_BOOTSTRAP="7"

# number of transactions each round
ENV NUM_TXN_PER_ROUND="10"

# the index of the node
ENV NODE_IDX="0"

# how long to run for before dying
# in minutes
ENV ONLINE_TIME="60"

# the seed to generate phaselock keys with
ENV SEED="1234"

# logging
ENV RUST_LOG="trace"

ENV PORT="9000"

EXPOSE 9000

CMD ["sh", "-c", "RUST_LOG=${RUST_LOG} /bin/multi-machine-libp2p --num_nodes=${NUM_NODES} --num_bootstrap=${NUM_BOOTSTRAP} --num_txn_per_round=${NUM_TXN_PER_ROUND} --node_idx=${NODE_IDX} --online_time=${ONLINE_TIME} --bound_addr=0.0.0.0:${PORT} --seed=${SEED} "]
