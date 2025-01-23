//! General (not HotShot-specific) utilities

/// A wrapper for a `JoinHandle` that will automatically abort the task if dropped
pub mod abort_on_drop_handle;
/// Error utilities, intended to function as a replacement to `anyhow`.
pub mod anytrace;
/// A bounded [`VecDeque`]
pub mod bounded_vec_deque;
