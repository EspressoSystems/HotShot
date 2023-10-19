#!/bin/bash
# Run any test repeatedly until failure is detected
# NOTE: test is hardcoded to be hotshot-testing and ten_tx_seven_nodes

counter=0

while true; do
  ((counter++))
  echo "Iteration: $counter"
  rm "output.json" || true
  just async_std test_basic >> "output.json" 2>&1
  error_code=$?
  if [ "$error_code" -ne 0 ]; then
    break
  fi
done
