#!/bin/bash

source "$HOME/.cargo/env"

# assign local ip
ip=`curl http://169.254.169.254/latest/meta-data/local-ipv4`
webserver_url=http://"$ip":9000
da_webserver_url=http://"$ip":9001
orchestrator_url=http://"$ip":4444

# start webserver
just async_std example webserver -- http://0.0.0.0:9000 &
just async_std example webserver -- http://0.0.0.0:9001 &
sleep 1m

# start orchestrator
just async_std example orchestrator-webserver -- --config_file ./crates/orchestrator/run-config.toml --orchestrator_url http://0.0.0.0:4444 --webserver_url ${webserver_url} --da_webserver_url ${da_webserver_url} &
sleep 1m

# start validators
docker build . -f ./docker/validator-webserver.Dockerfile -t ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
docker push ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
ecs deploy --region us-east-2 hotshot hotshot_centralized -i centralized ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
ecs deploy --region us-east-2 hotshot hotshot_centralized -c centralized ${orchestrator_url}
ecs scale --region us-east-2 hotshot hotshot_centralized 10 --timeout -1
sleep 2m #rethink about this

# kill them
ecs scale --region us-east-2 hotshot hotshot_centralized 0 --timeout -1
sleep 1m
for pid in $(ps -ef | grep "orchestrator" | awk '{print $2}'); do kill -9 $pid; done
for pid in $(ps -ef | grep "webserver" | awk '{print $2}'); do kill -9 $pid; done
