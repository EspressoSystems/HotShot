#!/usr/bin/env bash
# for i in {0..100}
# do
        printf "running iteration ${i}"
        rm file_*
        cargo run --features webui --example counter --release -- --bound_addr "127.0.0.1:9001" --node_type Bootstrap --num_nodes 4 --bootstrap "127.0.0.1:9001"  --webui 127.0.0.1:9091 > file_bs 2>&1 &
        sleep 1
        cargo run --features webui --example counter --release -- --bound_addr "127.0.0.1:9002" --node_type Regular --num_nodes 4 --bootstrap "127.0.0.1:9001"  --webui 127.0.0.1:9092 > file_reg_1 2>&1 &
        sleep 1

        cargo run --features webui --example counter --release -- --bound_addr "127.0.0.1:9003" --node_type Regular --num_nodes 4 --bootstrap "127.0.0.1:9001"  --webui 127.0.0.1:9093 > file_reg_2 2>&1 &
        sleep 1

        cargo run --features webui --example counter --release -- --bound_addr "127.0.0.1:9000" --node_type Conductor --num_nodes 4 --bootstrap "127.0.0.1:9001"  --webui 127.0.0.1:9090  > file_conductor 2>&1
# done
