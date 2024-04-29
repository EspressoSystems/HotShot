#!/bin/bash

# make sure the following is added to ~/.bashrc
source "$HOME/.cargo/env"

# assign local ip
ip=`curl http://169.254.169.254/latest/meta-data/local-ipv4`
orchestrator_url=http://"$ip":4444

# build
# `just async_std example validator-push-cdn -- http://127.0.0.1:4444` to get the bin in advance

# docker build and push
docker build . -f ./docker/validator-cdn.Dockerfile -t ghcr.io/espressosystems/hotshot/pushcdn-validator:6
docker push ghcr.io/espressosystems/hotshot/pushcdn-validator:6

# ecs deploy
ecs deploy --region us-east-2 hotshot hotshot_centralized -i centralized ghcr.io/espressosystems/hotshot/pushcdn-validator:6
ecs deploy --region us-east-2 hotshot hotshot_centralized -c centralized ${orchestrator_url}

# server1: broker and marshal
# server2: broker

# for a single run
# total_nodes, da_committee_size, transactions_per_round, transaction_size = 100, 10, 1, 4096
# for iteration of assignment
# see `aws_ecs_nginx_benchmarks.sh` for an example
for total_nodes in 10 50 100
do
    for da_committee_size in 10 50 100
    do
        if [ $da_committee_size -le $total_nodes ]
        then
            for transactions_per_round in 1 10 50 100
            do
                for transaction_size in 512 4096 # see large transaction size in aws_ecs_nginx_benchmarks.sh
                do
                    rounds=100
                    # runstart keydb
                    docker run --rm -p 0.0.0.0:6379:6379 eqalpha/keydb

                    # start orchestrator
                    just async_std example orchestrator-webserver -- --config_file ./crates/orchestrator/run-config.toml \
                                                                    --orchestrator_url http://0.0.0.0:4444 \
                                                                    --total_nodes ${total_nodes} \
                                                                    --da_committee_size ${da_committee_size} \
                                                                    --transactions_per_round ${transactions_per_round} \
                                                                    --transaction_size ${transaction_size} \
                                                                    --rounds ${rounds} \
                                                                    --commit_sha test &
                    sleep 30

                    # start validators
                    ecs scale --region us-east-2 hotshot hotshot_centralized ${total_nodes} --timeout -1
                    sleep $(($rounds + $total_nodes))

                    # kill them
                    ecs scale --region us-east-2 hotshot hotshot_centralized 0 --timeout -1
                    sleep 1m
                    for pid in $(ps -ef | grep "orchestrator" | awk '{print $2}'); do kill -9 $pid; done
                done
            done
        fi
    done
done
