#!/bin/bash

# make sure the following line is added to `~/.bashrc`
source "$HOME/.cargo/env"


if [ -z "$1" ]; then
    echo "No arguments provided. Usage: $0 <keydb_address> "
    exit 1
fi

echo "Argument 1: $1"
keydb_address="$1"

just async_std example cdn-broker -- -d $keydb_address \
                                    --public-bind-endpoint 0.0.0.0:1740 \
                                    --public-advertise-endpoint local_ip:1740 \
                                    --private-bind-endpoint 0.0.0.0:1741 \
                                    --private-advertise-endpoint local_ip:1741 &
