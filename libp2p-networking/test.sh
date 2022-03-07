#!/usr/bin/env bash
set -ex
RUST_LOG_FORMAT=none cargo test --release -- --test-threads=2
