use crate::H_256;
use blake3::Hasher;
use std::collections::HashSet;

pub use threshold_crypto as tc;

/// Seed for committee election, changed in each round.
pub type CommitteeSeed = [u8; H_256];

/// VRF output for committee election.
pub type CommitteeVrf = [u8; H_256];

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

/// Computes the VRF output for committee election associated with the signature.
fn compute_vrf(vrf_signature: &tc::SignatureShare) -> CommitteeVrf {
    let mut hasher = Hasher::new();
    hasher.update(&vrf_signature.to_bytes());
    *hasher.finalize().as_bytes()
}

/// Determines whether the hash of a seeded VRF should be selected.
///
/// A seeded VRF hash will be selected iff it's smaller than the hash selection threshold.
fn select_seeded_vrf_hash(
    seeded_vrf_hash: [u8; H_256],
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
fn select_seeded_vrf(
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

/// Gets the leader's ID given a list of committee members.
///
/// # Arguments
/// * `committee_members` - A list of tuples, each consisting of a member ID and a set of
/// selected VRF seeds.
#[allow(clippy::implicit_hasher)]
pub fn get_leader(committee_members: &[(u64, HashSet<u64>)]) -> Option<u64> {
    committee_members
        .iter()
        .max_by_key(|(_, vrf_seeds)| vrf_seeds.len())
        .map(|(leader_id, _)| *leader_id)
}

/// A structure for verifiable committee participation results.
pub struct VrfProof {
    /// The VRF signature share.
    pub signature: tc::SignatureShare,

    /// The set of stake such that `H(vrf | stake)` is selected.
    ///
    /// Stake is in the range of `[0, total_stake]`, where `total_stake` is a predetermined
    /// value representing the weight of the associated VRF public key, i.e., the maximum votes it
    /// may have. The size of the set is the actual number of votes granted in the current round.
    pub selected_stake: HashSet<u64>,
}

impl VrfProof {
    /// Signs the VRF signature and determines the participation.
    pub fn new(
        vrf_secret_key_share: &tc::SecretKeyShare,
        total_stake: u64,
        committee_seed: CommitteeSeed,
        selection_threshold: SelectionThreshold,
    ) -> Self {
        let signature = vrf_secret_key_share.sign(committee_seed);

        let vrf = compute_vrf(&signature);
        let mut selected_stake = HashSet::new();
        for stake in 0..total_stake {
            if select_seeded_vrf(&vrf, stake, selection_threshold) {
                selected_stake.insert(stake);
            }
        }

        Self {
            signature,
            selected_stake,
        }
    }

    /// Verifies the committee selection.
    ///
    /// # Errors
    /// Returns an error if:
    ///
    /// 1. the VRF signature is not the correct signature from the VRF public key and the
    /// committee seed, or
    ///
    /// 2. any `stake` in `selected_stake`:
    ///
    ///     2.1 is larger than the stake associated VRF public key, or
    ///
    ///     2.2 constructs an `H(vrf | vrf_seed)` that should not be selected.
    #[allow(clippy::implicit_hasher)]
    pub fn verify(
        &self,
        vrf_public_key: tc::PublicKeyShare,
        total_stake: u64,
        committee_seed: CommitteeSeed,
        selection_threshold: SelectionThreshold,
    ) -> Result<(), CommitteeError> {
        if !vrf_public_key.verify(&self.signature, committee_seed) {
            return Err(CommitteeError::IncorrectVrfSignature);
        }
        let vrf = compute_vrf(&self.signature);

        for stake in self.selected_stake.clone() {
            if stake >= total_stake {
                return Err(CommitteeError::InvaildVrfSeed);
            }
            if !select_seeded_vrf(&vrf, stake, selection_threshold) {
                return Err(CommitteeError::NotSelected);
            }
        }
        Ok(())
    }
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
        0, 1,
    ];

    // Test the verification of VRF output
    #[test]
    fn test_vrf_output() {
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

    // Test the selection of seeded VRF hash
    #[test]
    fn test_hash_selection() {
        let seeded_vrf_hash_1: SelectionThreshold = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let seeded_vrf_hash_2: SelectionThreshold = [
            128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ];
        let seeded_vrf_hash_3: SelectionThreshold = [
            128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 1,
        ];
        let seeded_vrf_hash_4: SelectionThreshold = [
            200, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 1,
        ];
        assert!(select_seeded_vrf_hash(
            seeded_vrf_hash_1,
            SELECTION_THRESHOLD
        ));
        assert!(select_seeded_vrf_hash(
            seeded_vrf_hash_2,
            SELECTION_THRESHOLD
        ));
        assert!(!select_seeded_vrf_hash(
            seeded_vrf_hash_3,
            SELECTION_THRESHOLD
        ));
        assert!(!select_seeded_vrf_hash(
            seeded_vrf_hash_4,
            SELECTION_THRESHOLD
        ));
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
        let selected_vrf_seeds = select_vrf(&vrf, STAKE, SELECTION_THRESHOLD);
        let selected_vrf_seeds_again = select_vrf(&vrf, STAKE, SELECTION_THRESHOLD);
        assert_eq!(selected_vrf_seeds, selected_vrf_seeds_again);
    }

    // Test the verification of VRF selection
    #[test]
    fn test_selection_verification() {
        // Generate keys
        let mut rng = Xoshiro256StarStar::seed_from_u64(SECRET_KEYS_SEED);
        let secret_keys = tc::SecretKeySet::random(THRESHOLD as usize - 1, &mut rng);
        let secret_key_share = secret_keys.secret_key_share(HONEST_NODE_ID);

        // Get the VRF selection results
        let signature = sign_vrf(&secret_key_share, COMMITTEE_SEED);
        let vrf = compute_vrf(&signature);
        let mut selected_vrf_seeds = select_vrf(&vrf, STAKE, SELECTION_THRESHOLD);

        // VRF verification should pass with the correct vrf seeds
        let verification =
            verify_selection(&vrf, selected_vrf_seeds.clone(), STAKE, SELECTION_THRESHOLD);
        assert!(verification.is_ok());

        // VRF verification should fail if any vrf seed is larger than the stake
        let verification =
            verify_selection(&vrf, selected_vrf_seeds.clone(), 10, SELECTION_THRESHOLD);
        assert_eq!(verification, Err(CommitteeError::InvaildVrfSeed));

        // VRF verification should fail if any vrf seed should not be selected
        selected_vrf_seeds.insert(50);
        let verification = verify_selection(&vrf, selected_vrf_seeds, STAKE, SELECTION_THRESHOLD);
        assert_eq!(verification, Err(CommitteeError::NotSelected));
    }
}
