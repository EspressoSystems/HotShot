//! configurable constants for hotshot
use std::num::NonZeroUsize;

use crate::data::TimeImpl;

/// replication factor for p
pub const DEFAULT_REPLICATION_FACTOR: Option<NonZeroUsize> = NonZeroUsize::new(20);

/// the genesis view number
pub const GENESIS_VIEW: TimeImpl = TimeImpl::new(0);

/// the number of views to gather information for ahead of time
pub const LOOK_AHEAD: u64 = 5;
