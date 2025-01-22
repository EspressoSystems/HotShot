//! General (not HotShot-specific) utilities

/// Error utilities, intended to function as a replacement to `anyhow`.
pub mod anytrace;
/// A wrapper for a `JoinHandle` that will automatically abort the task if dropped
pub mod abort_on_drop_handle;