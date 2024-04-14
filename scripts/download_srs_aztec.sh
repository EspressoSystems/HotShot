#!/usr/bin/env bash

set -euo pipefail

# Check if the .env file exists
if [ -f .env ]; then
    # Load variables from .env into the environment
    source .env
    echo "Environment variables loaded from .env"
else
    echo ".env file not found"
fi

if [ -f "$AZTEC_SRS_PATH" ]; then
    echo "SRS file $AZTEC_SRS_PATH exists"
else
    echo "SRS file $AZTEC_SRS_PATH does not exist, downloading ..."
    gh release download --repo alxiong/ark-srs v0.2.0 -p "$(basename $AZTEC_SRS_PATH)" -O "$AZTEC_SRS_PATH" --skip-existing
fi
