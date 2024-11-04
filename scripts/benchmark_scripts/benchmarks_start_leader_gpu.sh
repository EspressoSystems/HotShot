#!/bin/bash

# make sure the following line is added to `~/.bashrc`
source "$HOME/.cargo/env"

# Check if at least two arguments are provided
if [ $# -lt 2 ]; then
    echo "Usage: $0 <fixed_leader_for_gpuvid> <orchestrator_url>"
    exit 1
fi

echo "Argument 1: $1"
echo "Argument 2: $2"
fixed_leader_for_gpuvid="$1"
orchestrator_url="$2"

just example_gpuvid_leader multi-validator-push-cdn -- $fixed_leader_for_gpuvid $orchestrator_url &

                                