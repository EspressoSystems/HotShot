#!/bin/bash

# build
# REMEMBER to RUN
# source "$HOME/.cargo/env"
# just async_std example_fixed_leader validator-webserver -- http://127.0.0.1:4444
# in prior

# assign local ip
ip=`curl http://169.254.169.254/latest/meta-data/local-ipv4`
webserver_url=http://"$ip":9000
da_webserver_url=http://"$ip":9001
orchestrator_url=http://"$ip":4444

# docker build and push
docker build . -f ./docker/validator-webserver-local.Dockerfile -t ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
docker push ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std

# ecs deploy
ecs deploy --region us-east-2 hotshot hotshot_centralized -i centralized ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
ecs deploy --region us-east-2 hotshot hotshot_centralized -c centralized ${orchestrator_url}


# for a single run
# total_nodes, da_committee_size, transactions_per_round, transaction_size = 100, 10, 1, 4096
# for iteration of assignment
# see `aws_ecs_nginx_benchmarks.sh` for an example
for total_nodes in 10 50 100
do
    for da_committee_size in 5 10 50 100
    do
        if [ $da_committee_size -le $total_nodes ]
        then
            for transactions_per_round in 1 10 50 100
            do
                for transaction_size in 1000000 4096 #512 # see large transaction size in aws_ecs_nginx_benchmarks.sh
                do
                    for fixed_leader_for_gpuvid in 1 5 10
                    do
                        if [ $fixed_leader_for_gpuvid -le $da_committee_size ]
                        then
                            rounds=100
                            # start webserver
                            just async_std example webserver -- http://0.0.0.0:9000 &
                            just async_std example webserver -- http://0.0.0.0:9001 &
                            sleep 30

                            # start orchestrator
                            # just async_std example orchestrator-webserver -- --config_file ./crates/orchestrator/run-config.toml --orchestrator_url http://0.0.0.0:4444 --webserver_url http://172.31.7.196:9000 --da_webserver_url http://172.31.7.196:9001 --total_nodes 10 --da_committee_size 5 --transactions_per_round 1 --transaction_size 4096 --rounds 20 --fixed_leader_for_gpuvid 1 --commit_sha with_gpu
                            just async_std example_fixed_leader orchestrator-webserver -- --config_file ./crates/orchestrator/run-config.toml \
                                                                            --orchestrator_url http://0.0.0.0:4444 \
                                                                            --webserver_url ${webserver_url} \
                                                                            --da_webserver_url ${da_webserver_url} \
                                                                            --total_nodes ${total_nodes} \
                                                                            --da_committee_size ${da_committee_size} \
                                                                            --transactions_per_round ${transactions_per_round} \
                                                                            --transaction_size ${transaction_size} \
                                                                            --rounds ${rounds} \
                                                                            --fixed_leader_for_gpuvid ${fixed_leader_for_gpuvid} \
                                                                            --commit_sha fixed_leader_no_gpu_multi_server &
                            sleep 30
                            
                            # start leaders on GPU FIRST and WAIT for enough time till it registerred at orchestrator
                            # this step need to be done manually

                            # start validators
                            # Remember to minus  "- $fixed_leader_for_gpuvid" when doing gpu-vid
                            ecs scale --region us-east-2 hotshot hotshot_centralized $(($total_nodes)) --timeout -1
                            sleep $(($rounds + $total_nodes))

                            # kill them
                            ecs scale --region us-east-2 hotshot hotshot_centralized 0 --timeout -1
                            sleep 1m
                            for pid in $(ps -ef | grep "orchestrator" | awk '{print $2}'); do kill -9 $pid; done
                            for pid in $(ps -ef | grep "webserver" | awk '{print $2}'); do kill -9 $pid; done
                        fi
                    done
                done
            done
        fi
    done
done