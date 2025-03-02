#!/usr/bin/env sh

N_ITERATIONS=100
OUT_DIR="$(pwd)/flakiness"
TEST_THREADS=1

# Make script killable with Ctrl+C
trap "echo; exit" INT

die() {
    echo "$1";
    exit 1
}

# Parse arguments
while [ $# -gt 0 ]; do
  case $1 in
    -h|--help)
      echo "./flakiness -n [number of iterations] -o [output directory] -j [threads]"
      exit 0
      ;;
    -n)
      N_ITERATIONS="$2"
      shift # past argument
      shift # past value
      ;;
    -o)
      OUT_DIR="$2"
      shift # past argument
      shift # past value
      ;;
    -j)
      TEST_THREADS="$2"
      shift # past argument
      shift # past value
      ;;
    -*)
      echo "Unknown option: $1"
      exit 1
      ;;
  esac
done

# Don't clobber previous results
if [ -d "${OUT_DIR}" ]; then
  if [ "$(ls -A "${OUT_DIR}")" ]; then
    die "${OUT_DIR} is not empty"
  fi
fi

# Check for nextest
cargo nextest -V > /dev/null 2>&1 || die "cargo-nextest is not installed"

# Workaround for nextest in nix shell
DYLD_FALLBACK_LIBRARY_PATH=$(rustc --print sysroot)/lib
export DYLD_FALLBACK_LIBRARY_PATH

for ITERATION in $(seq -w "${N_ITERATIONS}");
do
    echo "Running iteration ${ITERATION}/${N_ITERATIONS}";
    export CARGO_TARGET_DIR="target_dirs/nix_rustc"
        if ! cargo nextest run \
            --no-fail-fast \
            --failure-output final \
            --test-threads "${TEST_THREADS}" \
            --hide-progress-bar > "${OUT_DIR}/${ITERATION}.log" 2>&1;
        then
        echo "${ITERATION}" >> "${OUT_DIR}/failed-iterations"
    fi
done
