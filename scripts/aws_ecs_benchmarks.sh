#!/bin/bash

source "$HOME/.cargo/env"

# assign local ip
ip=`curl http://169.254.169.254/latest/meta-data/local-ipv4`
webserver_url=http://"$ip":9000
da_webserver_url=http://"$ip":9001
orchestrator_url=http://"$ip":4444

# build
just async_std build
sleep 30s

# docker build and push
docker build . -f ./docker/validator-webserver-local.Dockerfile -t ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
docker push ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std

# ecs deploy
ecs deploy --region us-east-2 hotshot hotshot_centralized -i centralized ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
ecs deploy --region us-east-2 hotshot hotshot_centralized -c centralized ${orchestrator_url}

for total_nodes in 10 50
do
    rounds=100
    # start webserver
    just async_std example webserver -- http://0.0.0.0:9000 &
    just async_std example webserver -- http://0.0.0.0:9001 &
    sleep 1m

    # start orchestrator
    just async_std example orchestrator-webserver -- --config_file ./crates/orchestrator/run-config.toml \
                                                    --orchestrator_url http://0.0.0.0:4444 \
                                                    --webserver_url ${webserver_url} \
                                                    --da_webserver_url ${da_webserver_url} \
                                                    --total_nodes ${total_nodes} \
                                                    --da_committee_size 10 \
                                                    --transactions_per_round 100 \
                                                    --transaction_size 512 \
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
    for pid in $(ps -ef | grep "webserver" | awk '{print $2}'); do kill -9 $pid; done
done
