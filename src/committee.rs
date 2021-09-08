use crate::H_256;
use blake3::Hasher;
use std::collections::HashSet;
use std::hash::Hasher as HashSetHasher;

pub use threshold_crypto as tc;

/// Seed for committee election, changed in each round.
pub type CommitteeSeed = [u8; H_256];

/// VRF output for committee election.
pub type CommitteeVrf = [u8; H_256];

/// Hash of a seeded VRF.
pub type SeededVrfHash = [u8; H_256];

/// Selection threshold for the seeded VRF hash.
///
/// Should be constructed by `p * pow(2, 256)`, where `p` is the predetermined
/// probablistic of a stake being selected. A seeded VRF hash will be selected
/// iff it's smaller than the selection threshold.
pub type SelectionThreshold = [u8; H_256];

/// Error type for committee eleciton.
#[derive(Debug, PartialEq)]
pub enum CommitteeError {
    /// The VRF signature is not the correct signature from the public key and the message.
    IncorrectVrfSignature,

    /// The VRF seed exceeds stake.
    InvaildVrfSeed,

    /// The seeded VRF should not be elected.
    NotSelected,
}

/// Signs the VRF signature.
pub fn sign_vrf(
    vrf_secret_key_share: &tc::SecretKeyShare,
    committee_seed: CommitteeSeed,
) -> tc::SignatureShare {
    vrf_secret_key_share.sign(committee_seed)
}

/// Computes the VRF output for committee election associated with the signature.
pub fn compute_vrf(vrf_signature: &tc::SignatureShare) -> CommitteeVrf {
    let mut hasher = Hasher::new();
    hasher.update(&vrf_signature.to_bytes());
    *hasher.finalize().as_bytes()
}

/// Verifies a VRF signature and computes the VRF output.
///
/// # Errors
/// Returns an error if the VRF signature is not the correct signature from the VRF public key
/// and the committee seed.
pub fn verify_signature_and_compute_vrf(
    vrf_signature: &tc::SignatureShare,
    vrf_public_key: tc::PublicKeyShare,
    committee_seed: CommitteeSeed,
) -> Result<CommitteeVrf, CommitteeError> {
    if !vrf_public_key.verify(&vrf_signature, committee_seed) {
        return Err(CommitteeError::IncorrectVrfSignature);
    }
    Ok(compute_vrf(vrf_signature))
}

/// Determines whether the hash of a seeded VRF should be selected.
///
/// A seeded VRF hash will be selected iff it's smaller than the hash selection threshold.
pub fn select_seeded_vrf_hash(
    seeded_vrf_hash: SeededVrfHash,
    selection_threshold: SelectionThreshold,
) -> bool {
    seeded_vrf_hash < selection_threshold
}

/// Determines whether a seeded VRF should be selected.
///
/// # Arguments
///
/// * `vrf_seed` - The seed for hash calculation, in the range of `[0, stake]`, where
/// `stake` is a predetermined value representing the weight of the associated VRF public
/// key.
pub fn select_seeded_vrf(
    vrf: &CommitteeVrf,
    vrf_seed: u64,
    selection_threshold: SelectionThreshold,
) -> bool {
    let mut hasher = Hasher::new();
    hasher.update(vrf);
    hasher.update(&vrf_seed.to_be_bytes());
    let hash = *hasher.finalize().as_bytes();
    select_seeded_vrf_hash(hash, selection_threshold)
}

/// Determines the participation of a VRF.
///
/// Each VRF output is associated with a VRF public key. The number of votes a VRF public
/// key has is in the range of `[0, stake]`, where `stake` is a predetermined value
/// representing the weight of the associated VRF public key.
///
/// Returns the set of `vrf_seed`s such that `H(vrf | vrf_seed)` is selected.
pub fn select_vrf(
    vrf: &CommitteeVrf,
    stake: u64,
    selection_threshold: SelectionThreshold,
) -> HashSet<u64> {
    let mut selected_seeds = HashSet::new();
    for vrf_seed in 0..stake {
        if select_seeded_vrf(vrf, vrf_seed, selection_threshold) {
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
pub fn verify_selection<S: HashSetHasher>(
    vrf: &CommitteeVrf,
    selected_vrf_seeds: HashSet<u64, S>,
    stake: u64,
    selection_threshold: SelectionThreshold,
) -> Result<(), CommitteeError> {
    for vrf_seed in selected_vrf_seeds {
        if vrf_seed >= stake {
            return Err(CommitteeError::InvaildVrfSeed);
        }
        if !select_seeded_vrf(vrf, vrf_seed, selection_threshold) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};

    const SECRET_KEYS_SEED: u64 = 1234;
    const COMMITTEE_SEED: [u8; H_256] = [20; H_256];
    const INCORRECT_COMMITTEE_SEED: [u8; H_256] = [23; H_256];
    const THRESHOLD: u64 = 1000;
    const HONEST_NODE_ID: u64 = 30;
    const BYZANTINE_NODE_ID: u64 = 45;
    const STAKE: u64 = 55;
    const SELECTION_THRESHOLD: SelectionThreshold = [
        128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0,
    ];

    // Test VRF signing, computation and verification
    #[test]
    fn test_vrf_computation() {
        // Generate keys
        let mut rng = Xoshiro256StarStar::seed_from_u64(SECRET_KEYS_SEED);
        let secret_keys = tc::SecretKeySet::random(THRESHOLD as usize - 1, &mut rng);
        let secret_key_share_honest = secret_keys.secret_key_share(HONEST_NODE_ID);
        let secret_key_share_byzantine = secret_keys.secret_key_share(BYZANTINE_NODE_ID);
        let public_keys = secret_keys.public_keys();
        let public_key_honest = public_keys.public_key_share(HONEST_NODE_ID);
        let public_key_byzantine = public_keys.public_key_share(BYZANTINE_NODE_ID);

        // VRF verification should pass with the correct VRF signature and output
        let signature = sign_vrf(&secret_key_share_honest, COMMITTEE_SEED);
        let vrf = compute_vrf(&signature);
        let verification =
            verify_signature_and_compute_vrf(&signature, public_key_honest, COMMITTEE_SEED);
        assert_eq!(verification, Ok(vrf));

        // VRF verification should fail if the signature does not correspond to the public key
        let signature_byzantine = sign_vrf(&secret_key_share_byzantine, COMMITTEE_SEED);
        let verification = verify_signature_and_compute_vrf(
            &signature_byzantine,
            public_key_honest,
            COMMITTEE_SEED,
        );
        assert_eq!(verification, Err(CommitteeError::IncorrectVrfSignature));

        // VRF verification should fail if the signature does not correspond to the committee seed
        let signature_byzantine = sign_vrf(&secret_key_share_byzantine, INCORRECT_COMMITTEE_SEED);
        let verification = verify_signature_and_compute_vrf(
            &signature_byzantine,
            public_key_byzantine,
            COMMITTEE_SEED,
        );
        assert_eq!(verification, Err(CommitteeError::IncorrectVrfSignature));
    }

    // Test VRF selection
    #[test]
    fn test_vrf_selection() {
        // Generate keys
        let mut rng = Xoshiro256StarStar::seed_from_u64(SECRET_KEYS_SEED);
        let secret_keys = tc::SecretKeySet::random(THRESHOLD as usize - 1, &mut rng);
        let secret_key_share = secret_keys.secret_key_share(HONEST_NODE_ID);

        // Get the VRF output
        let signature = sign_vrf(&secret_key_share, COMMITTEE_SEED);
        let vrf = compute_vrf(&signature);

        // VRF selection should produces deterministic results
        let selected_vrf_seeds_1 = select_vrf(&vrf, STAKE, SELECTION_THRESHOLD);
        let selected_vrf_seeds_2 = select_vrf(&vrf, STAKE, SELECTION_THRESHOLD);
        assert_eq!(selected_vrf_seeds_1, selected_vrf_seeds_2);
    }
}
