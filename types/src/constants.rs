//! configurable constants for hotshot
use std::num::NonZeroUsize;

use crate::{data::TimeImpl, traits::signature_key::EncodedPublicKey};

/// replication factor for p
pub const DEFAULT_REPLICATION_FACTOR: Option<NonZeroUsize> = NonZeroUsize::new(20);

/// the genesis view number
pub const GENESIS_VIEW: TimeImpl = TimeImpl::new(0);

/// the number of views to gather information for ahead of time
pub const LOOK_AHEAD: u64 = 5;

/// the genesis proposer pk
/// unforutnately need to allocate so it's a function not a const
pub fn genesis_proposer_id() -> EncodedPublicKey {
 EncodedPublicKey(vec![4, 2])
}
