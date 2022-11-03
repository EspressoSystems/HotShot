//! configurable constants for hotshot
use std::num::NonZeroUsize;

use crate::traits::signature_key::EncodedPublicKey;

/// replication factor for p
pub const DEFAULT_REPLICATION_FACTOR: Option<NonZeroUsize> = NonZeroUsize::new(20);

/// the number of views to gather information for ahead of time
pub const LOOK_AHEAD: u64 = 5;

/// the genesis proposer pk
/// unfortunately need to allocate on the heap (for vec), so this ends up as a function instead of a
/// const
pub fn genesis_proposer_id() -> EncodedPublicKey {
    EncodedPublicKey(vec![4, 2])
}
