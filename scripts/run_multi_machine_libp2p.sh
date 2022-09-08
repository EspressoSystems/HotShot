#!/usr/bin/env bash

# example invocation of multimachine libp2p demo
# test for EST
BOOTSTRAP_ADDRS="0.us-east-2.cluster.aws.espresso.network:9000,1.us-east-2.cluster.aws.espresso.network:9000,2.us-east-2.cluster.aws.espresso.network:9000,3.us-east-2.cluster.aws.espresso.network:9000,4.us-east-2.cluster.aws.espresso.network:9000,5.us-east-2.cluster.aws.espresso.network:9000,6.us-east-2.cluster.aws.espresso.network:9000" BOUND_ADDR="0.0.0.0:9000" NUM_NODES="20" NUM_BOOTSTRAP="7" NUM_TXN_PER_ROUND="10" NODE_IDX="0" ONLINE_TIME="60" SEED="1234" RUST_LOG="warn" RUST_LOG_FORMAT="json" PORT="9000" BOOTSTRAP_MESH_N_HIGH="50" BOOTSTRAP_MESH_N_LOW="10" BOOTSTRAP_MESH_OUTBOUND_MIN="5" BOOTSTRAP_MESH_N="15" MESH_N_HIGH="15" MESH_N_LOW="8" MESH_OUTBOUND_MIN="4" MESH_N="12" START_TIMESTAMP="2022-09-08 07:35:00 -04:00:00" NEXT_VIEW_TIMEOUT="30" PROPOSE_MIN_ROUND_TIME="1" PROPOSE_MAX_ROUND_TIME="10" cargo run --release --example multi-machine-libp2p


