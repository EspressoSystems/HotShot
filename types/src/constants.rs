//! configurable constants for hotshot
use std::num::NonZeroUsize;

/// replication factor for p
pub const DEFAULT_REPLICATION_FACTOR: Option<NonZeroUsize> = NonZeroUsize::new(20);
