#!/bin/bash

# make sure the following line is added to `~/.bashrc`
source "$HOME/.cargo/env"

# assign local ip by curl from AWS metadata server:
# https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/instancedata-data-retrieval.html
ip=`curl http://169.254.169.254/latest/meta-data/local-ipv4`
orchestrator_url=http://"$ip":4444
cdn_marshal_address="$ip":9000
keydb_address=redis://"$ip":6379

# build to get the bin in advance, uncomment the following if built first time
just async_std example validator-push-cdn -- http://localhost:4444 &
# remember to sleep enough time if it's built first time
sleep 5m
for pid in $(ps -ef | grep "validator" | awk '{print $2}'); do kill -9 $pid; done

# docker build and push
docker build . -f ./docker/validator-cdn-local.Dockerfile -t ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
docker push ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std

# ecs deploy
ecs deploy --region us-east-2 hotshot hotshot_centralized -i centralized ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
ecs deploy --region us-east-2 hotshot hotshot_centralized -c centralized ${orchestrator_url} # http://172.31.8.82:4444

# runstart keydb
# docker run --rm -p 0.0.0.0:6379:6379 eqalpha/keydb &
# echo DEL brokers | keydb-cli
# server1 marshal
echo -e "\e[35mGoing to start cdn-marshal on local server\e[0m"
just async_std example cdn-marshal -- -d redis://localhost:6379 -b 9000 &
# remember to sleep enough time if it's built first time

# Function to round up to the nearest integer
round_up() {
  echo "scale=0;($1+0.5)/1" | bc
}

# for a single run
# total_nodes, da_committee_size, transactions_per_round, transaction_size = 100, 10, 1, 4096
# for iteration of assignment
# see `aws_ecs_benchmarks_webserver.sh` for an example
for total_nodes in 10 # 50 100 200 500 1000
do
    for da_committee_size in 5 # 10 50 100
    do
        if [ $da_committee_size -le $total_nodes ]
        then
            for transactions_per_round in 1 # 10 50 100
            do
                for transaction_size in 1000000 # 100000 1000000 10000000 20000000 # 512 4096
                do
                    for fixed_leader_for_gpuvid in 1 # 5 10 50 100
                    do
                        if [ $fixed_leader_for_gpuvid -le $da_committee_size ]
                        then
                            for rounds in 100 # 50
                            do
                                # server1 broker
                                echo -e "\e[35mGoing to start cdn-broker on local server\e[0m"
                                just async_std example cdn-broker -- -d redis://localhost:6379 \
                                    --public-bind-endpoint 0.0.0.0:1740 \
                                    --public-advertise-endpoint local_ip:1740 \
                                    --private-bind-endpoint 0.0.0.0:1741 \
                                    --private-advertise-endpoint local_ip:1741 &
                                # server2: broker
                                # make sure you're able to access the remote host from current host
                                # Sishan TODO:
                                # REMOTE_USER=[YOUR-ID]
                                # REMOTE_BROKER_HOST=[PUBLIC-IP]
                                echo -e "\e[35mGoing to start cdn-broker on remote server\e[0m"
                                REMOTE_USER="sishan"
                                REMOTE_BROKER_HOST="3.135.239.251"
                                COMMAND="./HotShot/scripts/benchmarks_start_cdn_broker.sh ${keydb_address}"
                                ssh $REMOTE_USER@$REMOTE_BROKER_HOST "$COMMAND exit"

                                # start orchestrator
                                echo -e "\e[35mGoing to start orchestrator on local server\e[0m"
                                just async_std example_fixed_leader orchestrator -- --config_file ./crates/orchestrator/run-config.toml \
                                                                                --orchestrator_url http://0.0.0.0:4444 \
                                                                                --total_nodes ${total_nodes} \
                                                                                --da_committee_size ${da_committee_size} \
                                                                                --transactions_per_round ${transactions_per_round} \
                                                                                --transaction_size ${transaction_size} \
                                                                                --rounds ${rounds} \
                                                                                --fixed_leader_for_gpuvid ${fixed_leader_for_gpuvid} \
                                                                                --cdn_marshal_address ${cdn_marshal_address} \
                                                                                --commit_sha random_tx &
                                sleep 30

                                # start leaders need to run on GPU FIRST
                                # and WAIT for enough time till it registerred at orchestrator
                                # make sure you're able to access the remote nvidia gpu server
                                echo -e "\e[35mGoing to start leaders on remote gpu server\e[0m"
                                REMOTE_GPU_HOST="18.220.24.72"
                                COMMAND_GPU_LEADER="./HotShot/scripts/benchmarks_start_leader_gpu.sh ${fixed_leader_for_gpuvid} ${orchestrator_url}"
                                ssh $REMOTE_USER@$REMOTE_GPU_HOST "$COMMAND_GPU_LEADER exit"
                                sleep 1m

                                # start validators
                                echo -e "\e[35mGoing to start validators on remote cpu servers\e[0m"
                                ecs scale --region us-east-2 hotshot hotshot_centralized $(($total_nodes - $fixed_leader_for_gpuvid)) --timeout -1
                                base=100
                                mul=$(echo "l($transaction_size * $transactions_per_round)/l($base)" | bc -l)
                                mul=$(round_up $mul)
                                sleep_time=$(( ($rounds + $total_nodes) * $mul ))
                                echo -e "\e[35msleep_time: $sleep_time\e[0m"
                                sleep $sleep_time

                                # kill them
                                # shut down nodes
                                echo -e "\e[35mGoing to stop validators on remote cpu servers\e[0m"
                                ecs scale --region us-east-2 hotshot hotshot_centralized 0 --timeout -1
                                # shut down leaders on gpu
                                echo -e "\e[35mGoing to stop leaders on remote gpu server\e[0m"
                                ssh $REMOTE_GPU_USER@$REMOTE_GPU_HOST "./HotShot/scripts/shutdown.sh exit"
                                echo -e "\e[35mGoing to stop orchestrator\e[0m"
                                for pid in $(ps -ef | grep "orchestrator" | awk '{print $2}'); do kill -9 $pid; done
                                # shut down brokers
                                echo -e "\e[35mGoing to stop cdn-broker\e[0m"
                                killall -9 cdn-broker
                                ssh $REMOTE_USER@$REMOTE_BROKER_HOST "./HotShot/scripts/shutdown.sh exit"
                                # remove brokers from keydb
                                # you'll need to do `echo DEL brokers | keydb-cli -a THE_PASSWORD` and set it to whatever password you set
                                echo DEL brokers | keydb-cli
                                # make sure you sleep at least 1 min to wait for DB to forget brokers and marshals
                                sleep $(( $total_nodes + 60))
                            done
                        fi
                    done
                done
            done
        fi
    done
done

# shut down all related threads
echo -e "\e[35mGoing to stop cdn-marshal\e[0m"
killall -9 cdn-marshal
# for pid in $(ps -ef | grep "keydb-server" | awk '{print $2}'); do sudo kill -9 $pid; done