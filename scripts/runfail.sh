#!/bin/bash

# Runs a command until it fails.
# Useful for running overnight to see if tests don't fail sporadically.
# 
# Usage:
# `./scripts/runfail.sh cargo test --profile=release-lto --features=full-ci --manifest-path testing/Cargo.toml`
$@

while [ $? -eq 0 ]; do
    $@
done
