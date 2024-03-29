#!/bin/bash

#standard
just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 10 --da_committee_size 10 --transactions_per_round 10 --transaction_size 512 --rounds 100 > scripts/benchmarks_results/run_10_10_10.output

#total_nodes and da_committee_size and transactions_per_round
just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 10 --da_committee_size 10 --transactions_per_round 100 --transaction_size 512 --rounds 100 > scripts/benchmarks_results/run_10_10_100.output
just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 20 --da_committee_size 5 --transactions_per_round 10 --transaction_size 512 --rounds 100  > scripts/benchmarks_results/run_20_5_10.output
just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 20 --da_committee_size 10 --transactions_per_round 10 --transaction_size 512 --rounds 100 > scripts/benchmarks_results/run_20_10_10.output
just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 20 --da_committee_size 20 --transactions_per_round 10 --transaction_size 512 --rounds 100 > scripts/benchmarks_results/run_20_20_10.output
# just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 50 --da_committee_size 10 --transactions_per_round 10 --transaction_size 512 --rounds 100
# just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 50 --da_committee_size 50 --transactions_per_round 10 --transaction_size 512 --rounds 100 
# just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 100 --da_committee_size 10 --transactions_per_round 10 --transaction_size 512 --rounds 100 
# just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 100 --da_committee_size 25 --transactions_per_round 10 --transaction_size 512 --rounds 100 
# just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 100 --da_committee_size 100 --transactions_per_round 10 --transaction_size 512 --rounds 100 
# just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 250 --da_committee_size 10 --transactions_per_round 10 --transaction_size 512 --rounds 100 
# just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 250 --da_committee_size 63 --transactions_per_round 10 --transaction_size 512 --rounds 100 
# just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 250 --da_committee_size 250 --transactions_per_round 10 --transaction_size 512 --rounds 100

#transaction_size
just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 10 --da_committee_size 10 --transactions_per_round 10 --transaction_size 4096 --rounds 100 > scripts/benchmarks_results/run_10_10_10_4096.output

#rounds
just async_std example all-webserver -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 10 --da_committee_size 10 --transactions_per_round 10 --transaction_size 512 --rounds 1000 > scripts/benchmarks_results/run_10_10_10_r1000.output

#networks: does not work for now
# just async_std example all-libp2p -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 10 --da_committee_size 10 --transactions_per_round 10 --transaction_size 512 --rounds 100 > scripts/benchmarks_results/run_10_10_10.output
# just async_std example all-combined -- --config_file ./crates/orchestrator/run-config.toml --total_nodes 10 --da_committee_size 10 --transactions_per_round 10 --transaction_size 512 --rounds 100 > scripts/benchmarks_results/run_10_10_10.output

