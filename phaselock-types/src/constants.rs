//! configurable constants for phaselock
use std::num::NonZeroUsize;

/// replication factor for p
pub const DEFAULT_REPLICATION_FACTOR: Option<NonZeroUsize> = NonZeroUsize::new(20);
