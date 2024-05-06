#!/bin/bash

# make sure the following line is added to `~/.bashrc`
source "$HOME/.cargo/env"

# assign local ip
ip=`curl http://169.254.169.254/latest/meta-data/local-ipv4`
orchestrator_url=http://"$ip":4444

# build
# `just async_std example validator-push-cdn -- http://127.0.0.1:4444` to get the bin in advance

# docker build and push
docker build . -f ./docker/validator-cdn.Dockerfile -t ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
docker push ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std

# ecs deploy
ecs deploy --region us-east-2 hotshot hotshot_centralized -i centralized ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
ecs deploy --region us-east-2 hotshot hotshot_centralized -c centralized ${orchestrator_url} # http://172.31.8.82:4444

# runstart keydb
# docker run --rm -p 0.0.0.0:6379:6379 eqalpha/keydb
# server1: broker and marshal
just async_std example cdn-marshal -- -d redis://localhost:6379 -b 9000 &
# remember to sleep enough time if it's built first time
just async_std example cdn-broker -- -d redis://localhost:6379 \
    --public-bind-endpoint 0.0.0.0:1740 \
    --public-advertise-endpoint local_ip:1740 \
    --private-bind-endpoint 0.0.0.0:1741 \
    --private-advertise-endpoint local_ip:1741 &
# server2: broker

# for a single run
# total_nodes, da_committee_size, transactions_per_round, transaction_size = 100, 10, 1, 4096
# for iteration of assignment
# see `aws_ecs_nginx_benchmarks.sh` for an example
for total_nodes in 10 # 50 100
do
    for da_committee_size in 5 #10 50 100
    do
        if [ $da_committee_size -le $total_nodes ]
        then
            for transactions_per_round in 1 # 10 50 100
            do
                for transaction_size in 512 4096 1000000
                do
                    for fixed_leader_for_gpuvid in 1 #5 10
                    do
                        if [ $fixed_leader_for_gpuvid -le $da_committee_size ]
                        then
                            rounds=100

                            # start orchestrator
                            just async_std example orchestrator-push-cdn -- --config_file ./crates/orchestrator/run-config.toml \
                                                                            --orchestrator_url http://0.0.0.0:4444 \
                                                                            --total_nodes ${total_nodes} \
                                                                            --da_committee_size ${da_committee_size} \
                                                                            --transactions_per_round ${transactions_per_round} \
                                                                            --transaction_size ${transaction_size} \
                                                                            --rounds ${rounds} \
                                                                            --fixed_leader_for_gpuvid ${fixed_leader_for_gpuvid} \
                                                                            --commit_sha cdn_no_gpu &
                            sleep 30

                            # start validators
                            ecs scale --region us-east-2 hotshot hotshot_centralized ${total_nodes} --timeout -1
                            sleep $(( ($rounds + $total_nodes) * $transactions_per_round ))

                            # kill them
                            ecs scale --region us-east-2 hotshot hotshot_centralized 0 --timeout -1
                            sleep 1m
                            for pid in $(ps -ef | grep "orchestrator" | awk '{print $2}'); do kill -9 $pid; done
                            sleep 10
                        fi
                    done
                done
            done
        fi
    done
done

# shut down all related threads
for pid in $(ps -ef | grep "cdn-broker" | awk '{print $2}'); do kill -9 $pid; done
for pid in $(ps -ef | grep "cdn-marshal" | awk '{print $2}'); do kill -9 $pid; done