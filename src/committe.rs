use crate::{data::BlockHash, H_256};
use blake3::Hasher;
use std::collections::HashSet;
use std::hash::Hasher as HashSetHasher;

pub use threshold_crypto as tc;

/// Error type for committe eleciton.
pub enum CommitteError {
    /// The VRF signature is not the correct signature from the public key and the message.
    IncorrectVrfSignature,

    /// The VRF output doesn not equal the hash of the VRF signature.
    IncorrectVrfOutput,

    /// The index of the seat exceeds the number of seats allowed.
    SeatIndexOverflow,

    /// The seat should not be elected.
    SeatNotElected,
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
/// 1. The VRF signature is not the correct signature from the VRF public key and the message.
/// 2. The VRF output doesn not equal the hash of the VRF signature.
pub fn verify_vrf(
    vrf: &CommitteVrf,
    vrf_signature: &tc::SignatureShare,
    vrf_public_key: tc::PublicKey,
    msg: BlockHash<H_256>,
) -> Result<(), CommitteError> {
    if !vrf_public_key.verify(&vrf_signature.0, msg) {
        return Err(CommitteError::IncorrectVrfSignature);
    }
    if compute_vrf(vrf_signature) != *vrf {
        return Err(CommitteError::IncorrectVrfOutput);
    }
    Ok(())
}

/// Gets the number of committe seats.
pub fn get_committe_size(total_stakes: u64) -> u64 {
    total_stakes * 2 / 3 + 1
}

/// Determines whether a seat of a VRF public key should be elected to the committe.
pub fn elect_seat(vrf: &CommitteVrf, i: u64, committe_size: u64, total_stakes: u64) -> bool {
    let mut hasher = Hasher::new();
    hasher.update(vrf);
    hasher.update(&i.to_be_bytes());
    let hash = *hasher.finalize().as_bytes();
    let mut hash_int: u64 = 0;
    for i in hash {
        hash_int = (hash_int << 8) + u64::from(i);
    }
    hash_int < total_stakes * (u64::pow(2, 256)) / committe_size
}

/// Determines the committe seats of a VRF public key.
///
/// Each VRF output is associated with a VRF public key. The number of seats a VRF public key
/// has is in the range of `[0, stakes]`, where `stakes` is a predetermined value representing
/// the weights of the VRF public key.
///
/// Returns the set of `i`s such that `H(vrf | i)` is elected.
pub fn elect_seats(vrf: &CommitteVrf, stakes: u64, total_stakes: u64) -> HashSet<u64> {
    let mut seats = HashSet::new();
    let committe_size = get_committe_size(total_stakes);
    for i in 0..stakes {
        if elect_seat(vrf, i, committe_size, total_stakes) {
            seats.insert(i);
        }
    }
    seats
}

/// Verifies the elected seats of a VRF public key.
///
/// # Errors
/// Returns an error if any `i` in `seats`:
/// 1. is larger than the number of seats allowed for the associated VRF public key, or
/// 2. constructs an `H(vrf | i)` that should not be elected.
pub fn verify_seats<S: HashSetHasher>(
    vrf: &CommitteVrf,
    seats: HashSet<u64, S>,
    stakes: u64,
    total_stakes: u64,
) -> Result<(), CommitteError> {
    let committe_size = get_committe_size(total_stakes);
    for i in seats {
        if i >= stakes {
            return Err(CommitteError::SeatIndexOverflow);
        }
        if !elect_seat(vrf, i, committe_size, total_stakes) {
            return Err(CommitteError::SeatNotElected);
        }
    }
    Ok(())
}
