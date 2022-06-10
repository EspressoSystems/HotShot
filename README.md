# PhaseLock Consensus Module

PhaseLock is a BFT consensus protocol based off of HotStuff, with the addition of proof-of-stake and
VRF committee elections.

# Usage

Please see the rustdoc for API documentation, and the examples directory for usage.

# Testing

To test:

```
RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT cargo test --verbose --release --lib --bins --tests --benches --all-features --workspace -- --nocapture --test-threads=1
```

## Testing on CI

The basic levels of logging include `warn`, `error`, `info`. The types of logging include `full`, `json`, and `compact`.

To test as if running on CI, one must limit the number of cores and ram to match github runners (2 core, 7 gig ram). To limit the ram, spin up a virtual machine or container with 7 gigs ram. To limit the core count when running tests:

```
ASYNC_STD_THREAD_COUNT=1 RUST_LOG=$ERROR_LOG_LEVEL RUST_LOG_FORMAT=$ERROR_LOG_FORMAT cargo test --verbose --release --lib --bins --tests --benches --all-features --workspace -- --nocapture --test-threads=1
```

# Git Workflow

See [here](./WORKFLOW.md)
