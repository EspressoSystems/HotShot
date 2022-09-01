use crate::H_256;
use blake3::Hasher;
use commit::{Commitment, RawCommitmentBuilder};
use hotshot_types::{
    data::ViewNumber,
    traits::{
        signature_key::{
            ed25519::{Ed25519Priv, Ed25519Pub},
            EncodedSignature, SignatureKey,
        },
        StateContents,
    },
};
use rand::Rng;
use rand_chacha::{rand_core::SeedableRng, ChaChaRng};
use std::{
    collections::{BTreeMap, HashSet},
    marker::PhantomData,
};

/// Determines whether the hash of a seeded VRF should be selected.
///
/// A seeded VRF hash will be selected iff it's smaller than the hash selection threshold.
fn select_seeded_vrf_hash(seeded_vrf_hash: [u8; H_256], selection_threshold: [u8; H_256]) -> bool {
    seeded_vrf_hash < selection_threshold
}

/// A trait for VRF proof, evaluation and verification.
pub trait Vrf<VrfHasher> {
    /// VRF public key.
    type PublicKey;

    /// VRF secret key.
    type SecretKey;

    /// VRF signature.
    type Proof;

    /// The input of VRF proof.
    type Input;

    /// The output of VRF evaluation.
    type Output;

    /// Creates the VRF proof associated with a VRF secret key.
    fn prove(secret_key: &Self::SecretKey, input: &Self::Input) -> Self::Proof;

    /// Computes the VRF output associated with a VRF proof.
    fn evaluate(proof: &Self::Proof) -> Self::Output;

    /// Verifies a VRF proof.
    fn verify(proof: Self::Proof, public_key: Self::PublicKey, input: Self::Input) -> bool;
}

/// A structure for dynamic committee.
pub struct DynamicCommittee<S, const N: usize> {
    /// State phantom.
    _state_phantom: PhantomData<S>,
}

impl<S, const N: usize> Vrf<Hasher> for DynamicCommittee<S, N> {
    type PublicKey = Ed25519Pub;
    type SecretKey = Ed25519Priv;
    type Proof = EncodedSignature;
    type Input = [u8; H_256];
    type Output = [u8; H_256];

    /// Signs the VRF signature.
    fn prove(vrf_secret_key: &Self::SecretKey, vrf_input: &Self::Input) -> Self::Proof {
        Ed25519Pub::sign(vrf_secret_key, vrf_input)
    }

    /// Computes the VRF output for committee election.
    fn evaluate(vrf_proof: &Self::Proof) -> Self::Output {
        let mut hasher = Hasher::new();
        hasher.update("VRF output".as_bytes());
        hasher.update(vrf_proof.as_ref());
        *hasher.finalize().as_bytes()
    }

    /// Verifies the VRF proof.
    #[allow(clippy::implicit_hasher)]
    fn verify(
        vrf_proof: Self::Proof,
        vrf_public_key: Self::PublicKey,
        vrf_input: Self::Input,
    ) -> bool {
        vrf_public_key.validate(&vrf_proof, &vrf_input)
    }
}

/// Stake table for `DynamicCommitee`
type StakeTable = BTreeMap<Ed25519Pub, u64>;

/// Constructed by `p * pow(2, 256)`, where `p` is the predetermined probabillity of a stake being
/// selected. A stake will be selected iff `H(vrf_output | stake)` is smaller than the selection
/// threshold.
type SelectionThreshold = [u8; H_256];

/// Vote token for [`DynamicCommittee`]
type VoteToken = EncodedSignature;

/// A tuple of a validated vote token and the associated selected stake.
type ValidatedVoteToken = (Ed25519Pub, EncodedSignature, HashSet<u64>);

impl<S, const N: usize> Default for DynamicCommittee<S, N> {
    fn default() -> Self {
        Self {
            _state_phantom: PhantomData,
        }
    }
}

impl<S: StateContents, const N: usize> DynamicCommittee<S, N> {
    /// Creates a new dynamic committee.
    pub fn new() -> Self {
        Self {
            _state_phantom: PhantomData,
        }
    }

    /// Hashes the view number and the next hash as the committee seed for vote token generation
    /// and verification.
    fn hash_commitee_seed(view_number: ViewNumber, next_state: Commitment<S>) -> [u8; H_256] {
        RawCommitmentBuilder::<S>::new("")
            .u64(*view_number)
            .var_size_bytes(next_state.as_ref())
            .finalize()
            .into()
    }

    /// Determines the number of votes a public key has.
    ///
    /// # Arguments
    ///
    /// - `stake` - The seed for hash calculation, in the range of `[0, total_stake]`, where
    /// `total_stake` is a predetermined value representing the weight of the associated VRF public
    /// key.
    #[allow(clippy::needless_pass_by_value)]
    fn select_stake(
        table: &StakeTable,
        selection_threshold: SelectionThreshold,
        pub_key: &Ed25519Pub,
        token: VoteToken,
    ) -> HashSet<u64> {
        let mut selected_stake = HashSet::new();

        let vrf_output = <Self as Vrf<Hasher>>::evaluate(&token);
        let total_stake = match table.get(pub_key) {
            Some(stake) => *stake,
            None => {
                return selected_stake;
            }
        };

        for stake in 0..total_stake {
            let mut hasher = Hasher::new();
            hasher.update("Seeded VRF".as_bytes());
            hasher.update(&vrf_output);
            hasher.update(&stake.to_be_bytes());
            let hash = *hasher.finalize().as_bytes();
            if select_seeded_vrf_hash(hash, selection_threshold) {
                selected_stake.insert(stake);
            }
        }

        selected_stake
    }

    /// Determines the leader.
    /// Note: A leader doesn't necessarily have to be a commitee member.
    pub fn get_leader(table: &StakeTable, view_number: ViewNumber) -> Ed25519Pub {
        let mut total_stake = 0;
        for record in table.iter() {
            total_stake += record.1;
        }

        let mut hasher = Hasher::new();
        hasher.update("Committee seed".as_bytes());
        hasher.update(&view_number.to_be_bytes());
        let hash = *hasher.finalize().as_bytes();
        let mut prng: ChaChaRng = SeedableRng::from_seed(hash);

        let selected_stake = prng.gen_range(0..total_stake);

        let mut stake_sum = 0;
        for record in table.iter() {
            stake_sum += record.1;
            if stake_sum > selected_stake {
                return *record.0;
            }
        }
        unreachable!()
    }

    // TODO !keyao Optimize VRF implementation with the sortition algorithm.
    // (Issue: <https://github.com/EspressoSystems/hotshot/issues/13>)

    /// Validates a vote token.
    ///
    /// Returns:
    /// * If the vote token isn't valid, the stake data isn't found, or the public key shouldn't be
    /// selected: null.
    /// * Otherwise: the validated tokan and the set of the selected stake, the size of which
    /// represents the number of votes.
    ///
    /// A stake is selected iff `H(vrf_output | stake) < selection_threshold`. Each stake is in the
    /// range of `[0, total_stake]`, where `total_stake` is a predetermined value representing the
    /// weight of the associated public key, i.e., the maximum votes it may have. The size of the
    /// set is the actual number of votes granted in the current round.
    pub fn get_votes(
        table: &StakeTable,
        selection_threshold: SelectionThreshold,
        view_number: ViewNumber,
        pub_key: Ed25519Pub,
        token: VoteToken,
        next_state: Commitment<S>,
    ) -> Option<ValidatedVoteToken> {
        let hash = Self::hash_commitee_seed(view_number, next_state);
        if !<Self as Vrf<Hasher>>::verify(token.clone(), pub_key, hash) {
            return None;
        }

        let selected_stake =
            Self::select_stake(table, selection_threshold, &pub_key, token.clone());

        if selected_stake.is_empty() {
            return None;
        }

        Some((pub_key, token, selected_stake))
    }

    /// Returns the number of votes a validated token has.
    pub fn get_vote_count(token: &ValidatedVoteToken) -> u64 {
        token.2.len() as u64
    }

    /// Attempts to generate a vote token for self.
    ///
    /// Returns null if the stake data isn't found or the number of votes is zero.
    pub fn make_vote_token(
        table: &StakeTable,
        selection_threshold: SelectionThreshold,
        view_number: ViewNumber,
        private_key: &Ed25519Priv,
        next_state: Commitment<S>,
    ) -> Option<VoteToken> {
        let hash = Self::hash_commitee_seed(view_number, next_state);
        let token = <Self as Vrf<Hasher>>::prove(private_key, &hash);

        let pub_key = Ed25519Pub::from_private(private_key);
        if !table.contains_key(&pub_key) {
            return None;
        }
        let selected_stake =
            Self::select_stake(table, selection_threshold, &pub_key, token.clone());

        if selected_stake.is_empty() {
            return None;
        }

        Some(token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use commit::RawCommitmentBuilder;
    use hotshot_types::traits::state::dummy::DummyState;
    use rand::RngCore;
    use std::collections::BTreeMap;

    type S = DummyState;
    const N: usize = H_256;
    const VIEW_NUMBER: ViewNumber = ViewNumber::new(10);
    const INCORRECT_VIEW_NUMBER: ViewNumber = ViewNumber::new(11);
    const NEXT_STATE: [u8; H_256] = [20; H_256];
    const INCORRECT_NEXT_STATE: [u8; H_256] = [22; H_256];
    const HONEST_NODE_ID: u64 = 30;
    const BYZANTINE_NODE_ID: u64 = 45;
    const STAKELESS_NODE_ID: u64 = 50;
    const TOTAL_STAKE: u64 = 55;
    const SELECTION_THRESHOLD: [u8; H_256] = [
        128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 1,
    ];

    fn with_seed(test: impl Fn([u8; 32])) {
        for _ in 0..100 {
            let mut seed = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut seed);
            println!("Using seed {:?}", seed);
            test(seed);
        }
    }

    // Helper function to construct a stake table
    fn dummy_stake_table(vrf_public_keys: &[Ed25519Pub]) -> BTreeMap<Ed25519Pub, u64> {
        let record_size = vrf_public_keys.len();
        let stake_per_record = TOTAL_STAKE / (record_size as u64);
        let last_stake = TOTAL_STAKE - stake_per_record * (record_size as u64 - 1);

        let mut stake_table = BTreeMap::new();
        #[allow(clippy::needless_range_loop)]
        for i in 0..record_size - 1 {
            stake_table.insert(vrf_public_keys[i], stake_per_record);
        }
        stake_table.insert(vrf_public_keys[record_size - 1], last_stake);

        stake_table
    }

    // Test the verification of VRF proof
    #[test]
    #[allow(clippy::shadow_unrelated)]
    fn test_vrf_verification() {
        with_seed(|seed| {
            // Generate keys
            let secret_key_honest = Ed25519Priv::generated_from_seed_indexed(seed, HONEST_NODE_ID);
            let public_key_honest = Ed25519Pub::from_private(&secret_key_honest);
            let secret_key_byzantine =
                Ed25519Priv::generated_from_seed_indexed(seed, BYZANTINE_NODE_ID);

            // VRF verification should pass with the correct secret key share, total stake, committee
            // seed, and selection threshold
            let next_state = RawCommitmentBuilder::new("")
                .fixed_size_bytes(&NEXT_STATE)
                .finalize();
            let input = DynamicCommittee::<S, N>::hash_commitee_seed(VIEW_NUMBER, next_state);
            let proof = DynamicCommittee::<S, N>::prove(&secret_key_honest, &input);
            let valid = DynamicCommittee::<S, N>::verify(proof.clone(), public_key_honest, input);
            assert!(valid);

            // VRF verification should fail if the secret key share does not correspond to the public
            // key share
            let incorrect_proof = DynamicCommittee::<S, N>::prove(&secret_key_byzantine, &input);
            let valid = DynamicCommittee::<S, N>::verify(incorrect_proof, public_key_honest, input);
            assert!(!valid);

            // VRF verification should fail if the view number used for proof generation is incorrect
            let incorrect_input =
                DynamicCommittee::<S, N>::hash_commitee_seed(INCORRECT_VIEW_NUMBER, next_state);
            let valid =
                DynamicCommittee::<S, N>::verify(proof.clone(), public_key_honest, incorrect_input);
            assert!(!valid);

            // VRF verification should fail if the next state used for proof generation is incorrect
            let incorrect_next_state = RawCommitmentBuilder::new("")
                .fixed_size_bytes(&INCORRECT_NEXT_STATE)
                .finalize();
            let incorrect_input =
                DynamicCommittee::<S, N>::hash_commitee_seed(VIEW_NUMBER, incorrect_next_state);
            let valid = DynamicCommittee::<S, N>::verify(proof, public_key_honest, incorrect_input);
            assert!(!valid);
        });
    }

    // Test the selection of seeded VRF hash
    #[test]
    fn test_hash_selection() {
        let seeded_vrf_hash_1 = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0,
        ];
        let seeded_vrf_hash_2 = [
            128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0,
        ];
        let seeded_vrf_hash_3 = [
            128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 1,
        ];
        let seeded_vrf_hash_4 = [
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

    // Test stake selection for member election
    #[test]
    fn test_stake_selection() {
        with_seed(|seed| {
            // Generate keys
            let secret_key_share = Ed25519Priv::generated_from_seed_indexed(seed, HONEST_NODE_ID);
            let pub_key = Ed25519Pub::from_private(&secret_key_share);
            let pub_keys = vec![pub_key];

            // Get the VRF proof
            let next_state = RawCommitmentBuilder::new("")
                .fixed_size_bytes(&NEXT_STATE)
                .finalize();
            let input = DynamicCommittee::<S, N>::hash_commitee_seed(VIEW_NUMBER, next_state);
            let proof = DynamicCommittee::<S, N>::prove(&secret_key_share, &input);

            // VRF selection should produce deterministic results
            let stake_table = dummy_stake_table(&pub_keys);
            let selected_stake = DynamicCommittee::<S, N>::select_stake(
                &stake_table,
                SELECTION_THRESHOLD,
                &pub_key,
                proof.clone(),
            );
            let selected_stake_again = DynamicCommittee::<S, N>::select_stake(
                &stake_table,
                SELECTION_THRESHOLD,
                &pub_key,
                proof,
            );
            assert_eq!(selected_stake, selected_stake_again);
        });
    }

    // Test leader selection
    #[test]
    fn test_leader_selection() {
        with_seed(|seed| {
            // Generate records
            let mut pub_keys = Vec::new();
            for i in 0..10 {
                let secret_key_share = Ed25519Priv::generated_from_seed_indexed(seed, i);
                let pub_key = Ed25519Pub::from_private(&secret_key_share);
                pub_keys.push(pub_key);
            }
            let stake_table = dummy_stake_table(&pub_keys);

            // Leader selection should produce deterministic results
            let selected_leader = DynamicCommittee::<S, N>::get_leader(&stake_table, VIEW_NUMBER);
            let selected_leader_again =
                DynamicCommittee::<S, N>::get_leader(&stake_table, VIEW_NUMBER);
            assert_eq!(selected_leader, selected_leader_again);
        });
    }

    // Test vote token generation and validation
    #[test]
    fn test_vote_token() {
        with_seed(|seed| {
            // Generate keys
            // let mut rng = Xoshiro256StarStar::seed_from_u64(SECRET_KEYS_SEED);
            let private_key_honest =
                { Ed25519Priv::generated_from_seed_indexed(seed, HONEST_NODE_ID) };
            let private_key_byzantine =
                { Ed25519Priv::generated_from_seed_indexed(seed, BYZANTINE_NODE_ID) };
            let private_key_stakeless =
                { Ed25519Priv::generated_from_seed_indexed(seed, STAKELESS_NODE_ID) };
            let pub_key_honest = Ed25519Pub::from_private(&private_key_honest);
            let pub_key_byzantine = Ed25519Pub::from_private(&private_key_byzantine);
            let pub_key_stakeless = Ed25519Pub::from_private(&private_key_stakeless);

            // Build the committee
            let mut stake_table = dummy_stake_table(&[pub_key_honest, pub_key_byzantine]);
            stake_table.insert(pub_key_stakeless, 0);

            // Vote token should be null if the public key is not selected as a member.
            let next_state = RawCommitmentBuilder::new("")
                .fixed_size_bytes(&NEXT_STATE)
                .finalize();
            let vote_token = DynamicCommittee::<S, N>::make_vote_token(
                &stake_table,
                SELECTION_THRESHOLD,
                VIEW_NUMBER,
                &private_key_stakeless,
                next_state,
            );
            assert!(vote_token.is_none());

            // Votes should be granted with the correct private key, view number, and next state
            let vote_token = DynamicCommittee::<S, N>::make_vote_token(
                &stake_table,
                SELECTION_THRESHOLD,
                VIEW_NUMBER,
                &private_key_honest,
                next_state,
            )
            .unwrap();
            let votes = DynamicCommittee::<S, N>::get_votes(
                &stake_table,
                SELECTION_THRESHOLD,
                VIEW_NUMBER,
                pub_key_honest,
                vote_token.clone(),
                next_state,
            );
            let validated_vote_token = votes.unwrap();
            assert_eq!(validated_vote_token.0, pub_key_honest);
            assert_eq!(validated_vote_token.1, vote_token);
            assert!(!validated_vote_token.2.is_empty());

            // No vote should be granted if the private key does not correspond to the public key
            let incorrect_vote_token = DynamicCommittee::<S, N>::make_vote_token(
                &stake_table,
                SELECTION_THRESHOLD,
                VIEW_NUMBER,
                &private_key_byzantine,
                next_state,
            )
            .unwrap();
            let votes = DynamicCommittee::<S, N>::get_votes(
                &stake_table,
                SELECTION_THRESHOLD,
                VIEW_NUMBER,
                pub_key_honest,
                incorrect_vote_token,
                next_state,
            );
            assert!(votes.is_none());

            // No vote should be granted if the view number used for token generation is incorrect
            let incorrect_vote_token = DynamicCommittee::<S, N>::make_vote_token(
                &stake_table,
                SELECTION_THRESHOLD,
                INCORRECT_VIEW_NUMBER,
                &private_key_honest,
                next_state,
            )
            .unwrap();
            let votes = DynamicCommittee::<S, N>::get_votes(
                &stake_table,
                SELECTION_THRESHOLD,
                VIEW_NUMBER,
                pub_key_honest,
                incorrect_vote_token,
                next_state,
            );
            assert!(votes.is_none());

            // No vote should be granted if the next state used for token generation is incorrect
            let incorrect_next_state = RawCommitmentBuilder::new("")
                .fixed_size_bytes(&INCORRECT_NEXT_STATE)
                .finalize();
            let incorrect_vote_token = DynamicCommittee::<S, N>::make_vote_token(
                &stake_table,
                SELECTION_THRESHOLD,
                VIEW_NUMBER,
                &private_key_honest,
                incorrect_next_state,
            )
            .unwrap();
            let votes = DynamicCommittee::<S, N>::get_votes(
                &stake_table,
                SELECTION_THRESHOLD,
                VIEW_NUMBER,
                pub_key_honest,
                incorrect_vote_token,
                next_state,
            );
            assert!(votes.is_none());
        });
    }
}
