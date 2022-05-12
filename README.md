# PhaseLock Consensus Module

PhaseLock is a BFT consensus protocol based off of HotStuff, with the addition of proof-of-stake and
VRF committee elections.

# Usage

Please see the rustdoc for API documentation, and the examples directory for usage.

# Tests

To run all tests:

```
cargo test --release --workspace -- --nocapture
```

To run all tests with backtrace debugging:

```
cargo test --profele=release-symbols --workspace
```

# Workflow

See [here](./WORKFLOW.md)
