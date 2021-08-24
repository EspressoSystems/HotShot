use crate::{data::BlockHash, H_256};
use blake3::Hasher;

pub use threshold_crypto as tc;

/// Error type for committe eleciton.
pub enum CommitteError {
    /// The VRF signature is not the correct signature from the public key and the message.
    IncorrectVrfSignature,

    /// The VRF output doesn not equal the hash of the VRF signature.
    IncorrectVrfOutput,
}

/// VRF output for committe election.
pub type CommitteVrf = [u8; H_256];

/// Signs the VRF signature.
pub fn sign_vrf(
    secret_key_share: &tc::SecretKeyShare,
    msg: BlockHash<H_256>,
) -> tc::SignatureShare {
    secret_key_share.sign(msg)
}

/// Computes the VRF output for committe election associated with the signature.
pub fn compute_vrf(vrf_signature: &tc::SignatureShare) -> CommitteVrf {
    let mut hasher = Hasher::new();
    hasher.update(&vrf_signature.to_bytes());
    *hasher.finalize().as_bytes()
}

/// Verifies VRF signature and output.
///
/// # Errors
/// Returns an error if either of the following:
/// 1. The VRF signature is not the correct signature from the public key and the message.
/// 2. The VRF output doesn not equal the hash of the VRF signature.
pub fn verify_vrf(
    vrf_signature: &tc::SignatureShare,
    vrf: &CommitteVrf,
    public_key: tc::PublicKey,
    msg: BlockHash<H_256>,
) -> Result<(), CommitteError> {
    if !public_key.verify(&vrf_signature.0, msg) {
        return Err(CommitteError::IncorrectVrfSignature);
    }
    if compute_vrf(vrf_signature) != *vrf {
        return Err(CommitteError::IncorrectVrfOutput);
    }
    Ok(())
}

/// Determines the election result for the specified round.
pub fn elect(
    vrf: &CommitteVrf,
    view: u64,
    committe_size: u64,
    total_size: u64,
    stakes: u64,
) -> bool {
    let mut hasher = Hasher::new();
    hasher.update(vrf);
    hasher.update(&view.to_be_bytes());
    let hash = *hasher.finalize().as_bytes();
    let mut hash_int: u64 = 0;
    for i in hash {
        hash_int = (hash_int << 8) + u64::from(i);
    }
    let hash_threshold = committe_size * stakes * (u64::pow(2, 256)) / total_size;
    hash_int < hash_threshold
}
