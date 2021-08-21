use crate::{H_256};
use blake3::Hasher;

pub use threshold_crypto as tc;

/// VRF value for committe election.
pub struct CommitteVrf(pub [u8; H_256]);

/// Caculates the VRF value for committe election associated with the signature in the specified round.
pub fn get_vrf(signature: &tc::SignatureShare, view: u64) -> CommitteVrf{
    let mut hasher = Hasher::new();
    hasher.update(&signature.to_bytes());
    hasher.update(&view.to_be_bytes());
    let hash = *hasher.finalize().as_bytes();
    CommitteVrf(hash)
}
