use crate::H_256;
use blake3::Hasher;
use std::collections::{HashMap, HashSet};

pub use threshold_crypto as tc;

/// Seed for committee election, changed in each round.
pub type CommitteeSeed = [u8; H_256];

/// VRF output for committee election.
pub type CommitteeVrf = [u8; H_256];

/// The threshold for stake selection.
///
/// Constructed by `p * pow(2, 256)`, where `p` is the predetermined probablistic of a stake
/// being selected. A stake will be selected iff `H(vrf | stake)` is smaller than the selection
/// threshold.
pub type SelectionThreshold = [u8; H_256];

/// Error type for committee eleciton.
#[derive(Debug, PartialEq)]
pub enum CommitteeError {
    /// The VRF signature is not the correct signature from the public key share and the message.
    IncorrectVrfSignature,

    /// The selected stake exceeds the total stake.
    InvaildStake,

    /// The stake should not be elected.
    NotSelected,
}

/// Computes the VRF output for committee election associated with the signature.
fn compute_vrf(vrf_signature: &tc::SignatureShare) -> CommitteeVrf {
    let mut hasher = Hasher::new();
    hasher.update(&vrf_signature.to_bytes());
    hasher.update("VRF output".as_bytes());
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
    hasher.update("Seeded VRF".as_bytes());
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

/// Committee records for verifying VRF proofs.
pub struct CommitteeRecords {
    /// A table mapping public key shares with the corresponding total stake.
    stake_table: HashMap<tc::PublicKeyShare, u64>,

    /// The threshold for stake selection.
    selection_threshold: SelectionThreshold,
}

/// A structure for verifiable committee participation results.
pub struct VrfProof {
    /// The VRF signature share.
    pub signature: tc::SignatureShare,

    /// The set of stake such that `H(vrf | stake)` is selected.
    ///
    /// Stake is in the range of `[0, total_stake]`, where `total_stake` is a predetermined
    /// value representing the weight of the associated VRF public key share, i.e., the maximum
    /// votes it may have. The size of the set is the actual number of votes granted in the current round.
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
    /// 1. the VRF signature is not the correct signature from the VRF public key share and the
    /// committee seed, or
    ///
    /// 2. any `stake` in `selected_stake`:
    ///
    ///     2.1 is larger than the total stake of the associated VRF public key share, or
    ///
    ///     2.2 constructs an `H(vrf | stake)` that should not be selected.
    #[allow(clippy::implicit_hasher)]
    pub fn verify(
        &self,
        vrf_public_key_share: tc::PublicKeyShare,
        committee_seed: CommitteeSeed,
        committee_records: &CommitteeRecords,
    ) -> Result<(), CommitteeError> {
        if !vrf_public_key_share.verify(&self.signature, committee_seed) {
            return Err(CommitteeError::IncorrectVrfSignature);
        }
        let vrf = compute_vrf(&self.signature);

        let total_stake = committee_records
            .stake_table
            .get(&vrf_public_key_share)
            .unwrap_or(&0);
        let selection_threshold = committee_records.selection_threshold;
        for stake in self.selected_stake.clone() {
            if stake >= *total_stake {
                return Err(CommitteeError::InvaildStake);
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
    const TOTAL_STAKE: u64 = 55;
    const SELECTION_THRESHOLD: SelectionThreshold = [
        128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 1,
    ];

    // Helper function to construct committee records
    fn dummy_committee_records(vrf_public_key_share: tc::PublicKeyShare) -> CommitteeRecords {
        let mut stake_table = HashMap::new();
        stake_table.insert(vrf_public_key_share, TOTAL_STAKE);
        CommitteeRecords {
            stake_table,
            selection_threshold: SELECTION_THRESHOLD,
        }
    }

    // Test the verification of VRF proof
    #[test]
    fn test_vrf_verification() {
        // Generate keys
        let mut rng = Xoshiro256StarStar::seed_from_u64(SECRET_KEYS_SEED);
        let secret_keys = tc::SecretKeySet::random(THRESHOLD as usize - 1, &mut rng);
        let secret_key_share_honest = secret_keys.secret_key_share(HONEST_NODE_ID);
        let secret_key_share_byzantine = secret_keys.secret_key_share(BYZANTINE_NODE_ID);
        let public_keys = secret_keys.public_keys();
        let public_key_share_honest = public_keys.public_key_share(HONEST_NODE_ID);
        let committee_records = dummy_committee_records(public_key_share_honest);

        // VRF verification should pass with the correct secret key share, total stake, committee seed,
        // and selection threshold
        let proof = VrfProof::new(
            &secret_key_share_honest,
            TOTAL_STAKE,
            COMMITTEE_SEED,
            SELECTION_THRESHOLD,
        );
        let verification =
            proof.verify(public_key_share_honest, COMMITTEE_SEED, &committee_records);
        assert!(verification.is_ok());

        // VRF verification should fail if the secret key share does not correspond to the public key share
        let proof = VrfProof::new(
            &secret_key_share_byzantine,
            TOTAL_STAKE,
            COMMITTEE_SEED,
            SELECTION_THRESHOLD,
        );
        let verification =
            proof.verify(public_key_share_honest, COMMITTEE_SEED, &committee_records);
        assert_eq!(verification, Err(CommitteeError::IncorrectVrfSignature));

        // VRF verification should fail if the committee seed used for proof generation is incorrect
        let proof = VrfProof::new(
            &secret_key_share_honest,
            TOTAL_STAKE,
            INCORRECT_COMMITTEE_SEED,
            SELECTION_THRESHOLD,
        );
        let verification =
            proof.verify(public_key_share_honest, COMMITTEE_SEED, &committee_records);
        assert_eq!(verification, Err(CommitteeError::IncorrectVrfSignature));

        // VRF verification should fail if any selected stake is larger than the total stake
        let mut proof = VrfProof::new(
            &secret_key_share_honest,
            TOTAL_STAKE,
            COMMITTEE_SEED,
            SELECTION_THRESHOLD,
        );
        proof.selected_stake.insert(56);
        let verification =
            proof.verify(public_key_share_honest, COMMITTEE_SEED, &committee_records);
        assert_eq!(verification, Err(CommitteeError::InvaildStake));

        // VRF verification should fail if any stake should not be selected
        let mut proof = VrfProof::new(
            &secret_key_share_honest,
            TOTAL_STAKE,
            COMMITTEE_SEED,
            SELECTION_THRESHOLD,
        );
        proof.selected_stake.insert(50);
        let verification =
            proof.verify(public_key_share_honest, COMMITTEE_SEED, &committee_records);
        assert_eq!(verification, Err(CommitteeError::NotSelected));
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
        let signature = secret_key_share.sign(COMMITTEE_SEED);
        let vrf = compute_vrf(&signature);

        // VRF selection should produces deterministic results
        let selected_vrf_seeds = select_seeded_vrf(&vrf, TOTAL_STAKE, SELECTION_THRESHOLD);
        let selected_vrf_seeds_again = select_seeded_vrf(&vrf, TOTAL_STAKE, SELECTION_THRESHOLD);
        assert_eq!(selected_vrf_seeds, selected_vrf_seeds_again);
    }
}
