//! configurable constants for hotshot
use std::num::NonZeroUsize;

use crate::data::ViewNumber;

/// replication factor for p
pub const DEFAULT_REPLICATION_FACTOR: Option<NonZeroUsize> = NonZeroUsize::new(20);

/// the genesis view number
pub const GENESIS_VIEW: ViewNumber = ViewNumber::new(0);
