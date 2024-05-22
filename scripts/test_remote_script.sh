#!/bin/bash

ip=`curl http://169.254.169.254/latest/meta-data/local-ipv4`
orchestrator_url=http://"$ip":4444
cdn_marshal_address="$ip":9000
keydb_address=redis://"$ip":6379

# Check if at least two arguments are provided
if [ $# -lt 2 ]; then
    echo "Usage: $0 <REMOTE_USER> <REMOTE_HOST>"
    exit 1
fi
REMOTE_USER="$1" #"sishan"
REMOTE_BROKER_HOST="$2" #"3.135.239.251"


echo -e "\e[35mGoing to start cdn-broker on remote server\e[0m"
ssh $REMOTE_USER@$REMOTE_BROKER_HOST << EOF
cd HotShot
nohup bash scripts/benchmarks_start_cdn_broker.sh ${keydb_address} > nohup.out 2>&1 &
exit
EOF

echo -e "\e[35mSleeping...\e[0m"
sleep 10

# shut down brokers
echo -e "\e[35mGoing to stop cdn-broker\e[0m"
ssh $REMOTE_USER@$REMOTE_BROKER_HOST "killall -9 cdn-broker && exit"