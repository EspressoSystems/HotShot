use crate::H_256;
use blake3::Hasher;
use std::collections::HashSet;
use std::hash::Hasher as HashSetHasher;

pub use threshold_crypto as tc;

/// Seed for committee election.
pub type CommitteeSeed = [u8; H_256];

/// VRF output for committee election.
pub type CommitteeVrf = [u8; H_256];

/// Error type for committee eleciton.
pub enum CommitteeError {
    /// The VRF signature is not the correct signature from the public key and the message.
    IncorrectVrfSignature,

    /// The VRF output doesn not equal the hash of the VRF signature.
    IncorrectVrfOutput,

    /// The VRF seed exceeds stake.
    InvaildVrfSeed,

    /// The seeded VRF should not be elected.
    NotSelected,
}

/// Signs the VRF signature.
pub fn sign_vrf(
    secret_key_share: &tc::SecretKeyShare,
    committee_seed: CommitteeSeed,
) -> tc::SignatureShare {
    secret_key_share.sign(committee_seed)
}

/// Computes the VRF output for committee election associated with the signature.
pub fn compute_vrf(vrf_signature: &tc::SignatureShare) -> CommitteeVrf {
    let mut hasher = Hasher::new();
    hasher.update(&vrf_signature.to_bytes());
    *hasher.finalize().as_bytes()
}

/// Verifies VRF signature and output.
///
/// # Errors
/// Returns an error if either of the following:
/// 1. The VRF signature is not the correct signature from the VRF public key and the committee seed.
/// 2. The VRF output doesn not equal the hash of the VRF signature.
pub fn verify_vrf(
    vrf: &CommitteeVrf,
    vrf_signature: &tc::SignatureShare,
    vrf_public_key: tc::PublicKey,
    committee_seed: CommitteeSeed,
) -> Result<(), CommitteeError> {
    if !vrf_public_key.verify(&vrf_signature.0, committee_seed) {
        return Err(CommitteeError::IncorrectVrfSignature);
    }
    if compute_vrf(vrf_signature) != *vrf {
        return Err(CommitteeError::IncorrectVrfOutput);
    }
    Ok(())
}

/// Determines whether a seeded VRF should be selected.
///
/// # Arguments
///
/// * `vrf_seed` - The seed for hash calculation, in the range of `[0, stake]`, where
/// `stake` is a predetermined value representing the weight of the associated VRF public
/// key.
///
/// * `committee_size` - The stake, rather than number of nodes, needed to form a committee.
pub fn select_seeded_vrf(
    vrf: &CommitteeVrf,
    vrf_seed: u64,
    total_stake: u64,
    committee_size: u64,
) -> bool {
    let mut hasher = Hasher::new();
    hasher.update(vrf);
    hasher.update(&vrf_seed.to_be_bytes());
    let hash = *hasher.finalize().as_bytes();
    let mut hash_int: u64 = 0;
    for i in hash {
        hash_int = (hash_int << 8) + u64::from(i);
    }
    hash_int < total_stake * (u64::pow(2, 256)) / committee_size
}

/// Determines the participation of a VRF.
///
/// Each VRF output is associated with a VRF public key. The number of votes a VRF public
/// key has is in the range of `[0, stake]`, where `stake` is a predetermined value
/// representing the weight of the associated VRF public key.
///
/// Returns the set of `vrf_seed`s such that `H(vrf | vrf_seed)` is selected.
///
/// # Arguments
/// * `committee_size` - The stake, rather than number of nodes, needed to form a
/// committee.
pub fn select_vrf(
    vrf: &CommitteeVrf,
    stake: u64,
    total_stake: u64,
    committee_size: u64,
) -> HashSet<u64> {
    let mut selected_seeds = HashSet::new();
    for vrf_seed in 0..stake {
        if select_seeded_vrf(vrf, vrf_seed, committee_size, total_stake) {
            selected_seeds.insert(vrf_seed);
        }
    }
    selected_seeds
}

/// Verifies the participation of a VRF.
///
/// # Errors
/// Returns an error if any `vrf_seed` in `selected_vrf_seeds`:
/// 1. is larger than the stake associated VRF public key, or
/// 2. constructs an `H(vrf | vrf_seed)` that should not be selected.
///
/// # Arguments
/// * `committee_size` - The stake, rather than number of nodes, needed to form a
/// committee.
pub fn verify_selection<S: HashSetHasher>(
    vrf: &CommitteeVrf,
    selected_vrf_seeds: HashSet<u64, S>,
    stake: u64,
    total_stake: u64,
    committee_size: u64,
) -> Result<(), CommitteeError> {
    for vrf_seed in selected_vrf_seeds {
        if vrf_seed >= stake {
            return Err(CommitteeError::InvaildVrfSeed);
        }
        if !select_seeded_vrf(vrf, vrf_seed, committee_size, total_stake) {
            return Err(CommitteeError::NotSelected);
        }
    }
    Ok(())
}

/// Gets the leader's ID given a list of committee members.
///
/// # Arguments
/// * `committee_members` - A list of tuples, each consisting of a member ID and a set of
/// selected VRF seeds.
pub fn get_leader<S: HashSetHasher>(committee_members: &[(u64, HashSet<u64, S>)]) -> Option<u64> {
    committee_members
        .iter()
        .max_by_key(|(_, vrf_seeds)| vrf_seeds.len())
        .map(|(leader_id, _)| *leader_id)
}
