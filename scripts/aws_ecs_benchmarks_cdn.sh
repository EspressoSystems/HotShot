#!/bin/bash

# make sure the following line is added to `~/.bashrc`
source "$HOME/.cargo/env"

# assign local ip by curl from AWS metadata server:
# https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/instancedata-data-retrieval.html
ip=`curl http://169.254.169.254/latest/meta-data/local-ipv4`
orchestrator_url=http://"$ip":4444
cdn_marshal_address="$ip":9000
keydb_address=redis://"$ip":6379

# Check if at least two arguments are provided
if [ $# -lt 2 ]; then
    echo "Usage: $0 <REMOTE_USER> <REMOTE_BROKER_HOST>"
    exit 1
fi
REMOTE_USER="$1" #"sishan"
REMOTE_BROKER_HOST="$2" #"3.135.239.251"

# this is to prevent "Error: Too many open files (os error 24). Pausing for 500ms"
ulimit -n 65536 
# build to get the bin in advance, uncomment the following if built first time
just async_std example_fixed_leader validator-push-cdn -- http://localhost:4444 &
# remember to sleep enough time if it's built first time
sleep 3m
for pid in $(ps -ef | grep "validator" | awk '{print $2}'); do kill -9 $pid; done

# # docker build and push
# docker build . -f ./docker/validator-cdn-local.Dockerfile -t ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
# docker push ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std

# # ecs deploy
# ecs deploy --region us-east-2 hotshot hotshot_centralized -i centralized ghcr.io/espressosystems/hotshot/validator-webserver:main-async-std
# ecs deploy --region us-east-2 hotshot hotshot_centralized -c centralized ${orchestrator_url} # http://172.31.8.82:4444

# runstart keydb
# docker run --rm -p 0.0.0.0:6379:6379 eqalpha/keydb &
# server1: marshal
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
for total_nodes in 10 50 100 200 500 1000
do
    for da_committee_size in 5 10 50 100
    do
        if [ $da_committee_size -le $total_nodes ]
        then
            for transactions_per_round in 1 10 50 # 100
            do
                for transaction_size in 100000 1000000 10000000 20000000 # 512 4096
                do
                    for fixed_leader_for_gpuvid in 1 5 10 50 100
                    do
                        if [ $fixed_leader_for_gpuvid -le $da_committee_size ]
                        then
                            for rounds in 100
                            do
                                # server1: broker
                                echo -e "\e[35mGoing to start cdn-broker on local server\e[0m"
                                just async_std example cdn-broker -- -d redis://localhost:6379 \
                                    --public-bind-endpoint 0.0.0.0:1740 \
                                    --public-advertise-endpoint local_ip:1740 \
                                    --private-bind-endpoint 0.0.0.0:1741 \
                                    --private-advertise-endpoint local_ip:1741 &
                                # server2: broker
                                # make sure you're able to access the remote host from current host
                                echo -e "\e[35mGoing to start cdn-broker on remote server\e[0m"
                                ssh $REMOTE_USER@$REMOTE_BROKER_HOST << EOF
cd HotShot
nohup bash scripts/benchmarks_start_cdn_broker.sh ${keydb_address} > nohup.out 2>&1 &
exit
EOF

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
                                                                                --commit_sha cdn_simple_builder_fixed_leader &
                                sleep 30

                                # start validators
                                echo -e "\e[35mGoing to start validators on remote servers\e[0m"
                                ecs scale --region us-east-2 hotshot hotshot_centralized ${total_nodes} --timeout -1
                                base=100
                                mul=$(echo "l($transaction_size * $transactions_per_round)/l($base)" | bc -l)
                                mul=$(round_up $mul)
                                sleep_time=$(( ($rounds + $total_nodes / 2 ) * $mul ))
                                echo -e "\e[35msleep_time: $sleep_time\e[0m"
                                sleep $sleep_time

                                # kill them
                                echo -e "\e[35mGoing to stop validators on remote servers\e[0m"
                                ecs scale --region us-east-2 hotshot hotshot_centralized 0 --timeout -1
                                for pid in $(ps -ef | grep "orchestrator" | awk '{print $2}'); do kill -9 $pid; done
                                # shut down brokers
                                echo -e "\e[35mGoing to stop cdn-broker\e[0m"
                                killall -9 cdn-broker
                                ssh $REMOTE_USER@$REMOTE_BROKER_HOST "killall -9 cdn-broker && exit"
                                # remove brokers from keydb
                                # you'll need to do `echo DEL brokers | keydb-cli -a THE_PASSWORD` and set it to whatever password you set
                                echo DEL brokers | keydb-cli
                                # make sure you sleep at least 1 min
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