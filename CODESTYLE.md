# HotShot Code Style Guide

## Logging Guidelines

### Debug
Use `debug!` for routine events that occur frequently within the system.

Example:
```rust
debug!(
    "View number={} decided",
    view_number
);
```

### Info
Use `info!` for events that occur under specific conditions, which are not issues but might aid in debugging.

Example:
```rust
if missing_data {
    info!(
        "Fetching missing data: query_id={}",
        query_id
    );
}
```

### Warn
Use `warn!` for events that indicate a potential issue, which the system can handle, but might require human attention.

Example:
```rust
if message_loss_rate > threshold {
    warn!(
        "Increased message loss detected: rate={}, threshold={}",
        message_loss_rate,
        threshold
    );
}
```

### Error
Use `error!` for critical issues that could lead to a permanent degradation of the system without manual intervention.

Example, we log an error when safety and liveness are violated:
```rust
if !safety_check && !liveness_check {
    error!(
        "Failed safety and liveness check: High QC={:?}, Proposal QC={:?}, Locked view={:?}",
        consensus.high_qc(),
        proposal.data,
        consensus.locked_view()
    );
}
```
