#!/bin/bash

source "$HOME/.cargo/env"

# assign local ip
# these are fixed because nginx config is fixed in other servers 
# which we are not able to access and update in this script.
# ip=`curl http://169.254.169.254/latest/meta-data/local-ipv4`
webserver_url=http://172.31.28.184:80
da_webserver_url=http://172.31.44.172:81
orchestrator_url=http://172.31.8.82:4444 #http://172.31.7.196:5555  if using nginx for orchestrator


# build
just async_std build
sleep 30s

# docker build and push
docker build . -f ./docker/validator-webserver-local.Dockerfile -t ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
docker push ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std

# ecs deploy
ecs deploy --region us-east-2 hotshot hotshot_centralized -i centralized ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
ecs deploy --region us-east-2 hotshot hotshot_centralized -c centralized ${orchestrator_url}

# start these two dockers in another two servers and keep them running
# enter the repo hotshot-nginx and switch to sishan/autobench
# docker build . -f Dockerfile -t sishan-nginx
# docker run --network=host sishan-nginx:latest

# docker build . -f Dockerfile_da -t sishan-nginx
# docker run --network=host sishan-nginx:latest

# start this docker if using orchestrator with nginx
# docker build . -f Dockerfile_orchestrator -t sishan-nginx
# docker run --network=host sishan-nginx:latest

OLDIFS=$IFS; IFS=',';
for config in 10,5,1,10000000,20 50,5,1,1000000,20 10,5,1,20000000,20 #100,10,1,20000000,20 200,10,1,20000000,20
do
    set -- $config;
    # start webserver
    just async_std example webserver -- http://0.0.0.0:9000 &
    just async_std example webserver -- http://0.0.0.0:9001 &
    sleep 30

    # start orchestrator
    #just async_std example orchestrator-webserver -- --config_file ./crates/orchestrator/run-config.toml --orchestrator_url http://0.0.0.0:4444 --webserver_url http://172.31.28.184:80 --da_webserver_url http://172.31.44.172:81 --total_nodes 10 --da_committee_size 5 --transactions_per_round 1 --transaction_size 1000000 --rounds 20 --commit_sha test_orchestrator
    just async_std example orchestrator-webserver -- --config_file ./crates/orchestrator/run-config.toml \
                                                    --orchestrator_url http://0.0.0.0:4444 \
                                                    --webserver_url ${webserver_url} \
                                                    --da_webserver_url ${da_webserver_url} \
                                                    --total_nodes $1 \
                                                    --da_committee_size $2 \
                                                    --transactions_per_round $3 \
                                                    --transaction_size $4 \
                                                    --rounds $5 \
                                                    --commit_sha nginx_script_test &
    sleep 30

    # start validators
    ecs scale --region us-east-2 hotshot hotshot_centralized $1 --timeout -1
    sleep $((($5 + $1) * 5))
    sleep 10m

    # kill them
    ecs scale --region us-east-2 hotshot hotshot_centralized 0 --timeout -1
    sleep 1m
    for pid in $(ps -ef | grep "orchestrator" | awk '{print $2}'); do kill -9 $pid; done
    for pid in $(ps -ef | grep "webserver" | awk '{print $2}'); do kill -9 $pid; done
done
IFS=$OLDIFS
