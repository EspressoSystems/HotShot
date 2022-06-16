#!/usr/bin/env bash

printf "running iteration"
rm file_*
cargo run --features webui --example counter --release -- --bound_addr "127.0.0.1:9001" --node_type Bootstrap --num_nodes 103 --bootstrap "127.0.0.1:9001,127.0.0.1:9002"  --num_gossip 300 --webui 127.0.0.1:9091 > file_bs 2>&1 &
sleep 1
cargo run --features webui --example counter --release -- --bound_addr "127.0.0.1:9002" --node_type Bootstrap --num_nodes 103 --bootstrap "127.0.0.1:9001,127.0.0.1:9002"  --num_gossip 300 --webui 127.0.0.1:9091 > file_bs 2>&1 &
sleep 1
for i in {100..200}
do
        cargo run --features webui --example counter --release -- --bound_addr "127.0.0.1:9${i}" --node_type Regular --num_nodes 103 --bootstrap "127.0.0.1:9001,127.0.0.1:9002"  --num_gossip 300 --webui 127.0.0.1:9092 > file_regular_${i} 2>&1 &

done

sleep 5
cargo run --features webui --example counter --release -- --bound_addr "127.0.0.1:9000" --node_type Conductor --num_nodes 103 --bootstrap "127.0.0.1:9001,127.0.0.1:9002"  --num_gossip 300 --webui 127.0.0.1:9090  > file_conductor 2>&1
