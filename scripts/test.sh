#!/bin/bash
# Run any test repeatedly until failure is detected
# NOTE: test is hardcoded to be hotshot-testing and ten_tx_seven_nodes

counter=0

while true; do
  ((counter++))
  echo "Iteration: $counter"
  rm "test_log.txt" || true
  just test_async_std_pkg_test hotshot-testing ten_tx_seven_nodes >> "test_log.txt" 2>&1
  error_code=$?
  if [ "$error_code" -ne 0 ]; then
    break
  fi
done
