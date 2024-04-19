#!/usr/bin/env bash

set -euo pipefail

# Check if the current directory is "HotShot"
if [ "$(basename "$(pwd)")" != "HotShot" ]; then
    echo "Error: This script must be run from the 'HotShot' directory."
    exit 1
fi

# Ensure the 'data' directory exists
if [ ! -d "data" ]; then
    echo "Creating 'data' directory..."
    mkdir -p data
fi

export AZTEC_SRS_PATH="$(pwd)/data/aztec20/kzg10-aztec20-srs-1048578.bin"
if [ -f "$AZTEC_SRS_PATH" ]; then
    echo "SRS file $AZTEC_SRS_PATH exists"
else
    echo "SRS file $AZTEC_SRS_PATH does not exist, downloading ..."
    wget -q -P "$(dirname $AZTEC_SRS_PATH)" "https://github.com/EspressoSystems/ark-srs/releases/download/v0.2.0/$(basename $AZTEC_SRS_PATH)"
fi
