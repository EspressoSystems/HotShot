use ark_ec::bls12::Bls12Parameters;
use bincode::Options;
use hotshot_types::{traits::{
    election::{Checked, Election, ElectionError, VoteToken},
    signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
    state::ConsensusTime,
    State,
}, data::ViewNumber};
use hotshot_utils::bincode::bincode_opts;
use jf_primitives::{
    hash_to_group::SWHashToGroup,
    signatures::{bls::BLSVerKey, BLSSignatureScheme, SignatureScheme},
    vrf::Vrf,
};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use serde::{
    de::{self, DeserializeOwned},
    Deserialize, Serialize,
};
use std::{
    collections::BTreeMap,
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
    num::NonZeroU64,
    sync::{Arc, Mutex},
};

use jf_primitives::{signatures::{
    bls::{BLSSignKey, BLSSignature},
}};
use num::{rational::Ratio, FromPrimitive, BigUint};

// TODO wrong palce for this
pub const SORTITION_PARAMETER: u64 = 100;

// TODO abstraction this function's impl into a trait
// TODO do we necessariy want the units of stake to be a u64? or generics
#[derive(Serialize, Deserialize)]
pub struct VRFStakeTable<VRF, VRFHASHER, VRFPARAMS> {
    mapping: BTreeMap<EncodedPublicKey, NonZeroU64>,
    total_stake: u64,
    _pd_0: PhantomData<VRF>,
    _pd_1: PhantomData<VRFHASHER>,
    _pd_2: PhantomData<VRFPARAMS>,
}

impl<VRF, VRFHASHER, VRFPARAMS> Clone for VRFStakeTable<VRF, VRFHASHER, VRFPARAMS> {
    fn clone(&self) -> Self {
        Self {
            mapping: self.mapping.clone(),
            total_stake: self.total_stake,
            _pd_0: PhantomData,
            _pd_1: PhantomData,
            _pd_2: PhantomData,
        }
    }
}

#[derive(Deserialize, Serialize)]
pub struct VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme,
    SIGSCHEME::VerificationKey: Clone,
{
    pk: SIGSCHEME::VerificationKey,
    _pd_0: PhantomData<SIGSCHEME::SigningKey>,
}

impl<SIGSCHEME> VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme,
    SIGSCHEME::VerificationKey: Clone,
{
    pub fn from_native(pk: SIGSCHEME::VerificationKey) -> Self {
        Self {
            pk,
            _pd_0: PhantomData,
        }
    }
}

impl<SIGSCHEME> Clone for VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme,
    SIGSCHEME::VerificationKey: Clone,
{
    fn clone(&self) -> Self {
        Self {
            pk: self.pk.clone(),
            _pd_0: PhantomData,
        }
    }
}

impl<SIGSCHEME> Debug for VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme<PublicParameter = (), MessageUnit = u8>,
    SIGSCHEME::VerificationKey: Clone + for<'a> Deserialize<'a> + Serialize + Send + Sync,
    SIGSCHEME::SigningKey: Clone + for<'a> Deserialize<'a> + Serialize + Send + Sync,
    SIGSCHEME::Signature: Clone + for<'a> Deserialize<'a> + Serialize,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("VRFPubKey").field(&self.to_bytes()).finish()
    }
}
impl<SIGSCHEME> PartialEq for VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme<PublicParameter = (), MessageUnit = u8>,
    SIGSCHEME::VerificationKey: Clone + for<'a> Deserialize<'a> + Serialize + Send + Sync,
    SIGSCHEME::SigningKey: Clone + for<'a> Deserialize<'a> + Serialize + Send + Sync,
    SIGSCHEME::Signature: Clone + for<'a> Deserialize<'a> + Serialize,
{
    fn eq(&self, other: &Self) -> bool {
        self.to_bytes() == other.to_bytes()
    }

    fn ne(&self, other: &Self) -> bool {
        !self.eq(other)
    }
}
impl<SIGSCHEME> Eq for VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme<PublicParameter = (), MessageUnit = u8>,
    SIGSCHEME::VerificationKey: Clone + for<'a> Deserialize<'a> + Serialize + Send + Sync,
    SIGSCHEME::SigningKey: Clone + for<'a> Deserialize<'a> + Serialize + Send + Sync,
    SIGSCHEME::Signature: Clone + for<'a> Deserialize<'a> + Serialize,
{
}
impl<SIGSCHEME> Hash for VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme<PublicParameter = (), MessageUnit = u8>,
    SIGSCHEME::VerificationKey: Clone + de::DeserializeOwned + Serialize + Send + Sync,
    SIGSCHEME::SigningKey: Clone + de::DeserializeOwned + Serialize + Send + Sync,
    SIGSCHEME::Signature: Clone + for<'a> Deserialize<'a> + Serialize,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.to_bytes().hash(state);
    }
}
impl<SIGSCHEME> SignatureKey for VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme<PublicParameter = (), MessageUnit = u8>,
    SIGSCHEME::VerificationKey: Clone + for<'a> Deserialize<'a> + Serialize + Send + Sync,
    SIGSCHEME::SigningKey: Clone + for<'a> Deserialize<'a> + Serialize + Send + Sync,
    SIGSCHEME::Signature: Clone + for<'a> Deserialize<'a> + Serialize,
{
    type PrivateKey = (SIGSCHEME::SigningKey, SIGSCHEME::VerificationKey);

    fn validate(&self, signature: &EncodedSignature, data: &[u8]) -> bool {
        let x: Result<SIGSCHEME::Signature, _> = bincode_opts().deserialize(&signature.0);
        match x {
            Ok(s) => {
                // First hash the data into a constant sized digest
                SIGSCHEME::verify(&(), &self.pk, data, &s).is_ok()
            }
            Err(_) => false,
        }
    }

    fn sign(private_key: &Self::PrivateKey, data: &[u8]) -> EncodedSignature {
        // Sign it
        let signature = SIGSCHEME::sign(&(), &private_key.0, data, &mut rand::thread_rng())
            .expect("This signature shouldn't be able to fail");
        // Encode it
        let bytes = bincode_opts()
            .serialize(&signature)
            .expect("This serialization shouldn't be able to fail");
        EncodedSignature(bytes)
    }

    fn from_private(private_key: &Self::PrivateKey) -> Self {
        Self {
            pk: private_key.1.clone(),
            _pd_0: PhantomData,
        }
    }

    fn to_bytes(&self) -> hotshot_types::traits::signature_key::EncodedPublicKey {
        EncodedPublicKey(
            bincode_opts()
                .serialize(&self.pk)
                .expect("Serialization should not be able to fail"),
        )
    }

    fn from_bytes(bytes: &hotshot_types::traits::signature_key::EncodedPublicKey) -> Option<Self> {
        bincode_opts().deserialize(&bytes.0).ok().map(|pk| Self {
            pk,
            _pd_0: PhantomData,
        })
    }
    fn generated_from_seed_indexed(seed: [u8; 32], index: u64) -> (Self, Self::PrivateKey) {
        let mut prng = rand::thread_rng();

        let param = SIGSCHEME::param_gen(Some(&mut prng)).unwrap();
        let (sk, pk) = SIGSCHEME::key_gen(&param, &mut prng).unwrap();
        (
            Self {
                pk: pk.clone(),
                _pd_0: PhantomData,
            },
            (sk, pk),
        )
    }
}

impl<VRF, VRFHASHER, VRFPARAMS> VRFStakeTable<VRF, VRFHASHER, VRFPARAMS> {
    pub fn get_all_stake(&self) -> u64 {
        self.total_stake
    }
}

impl<VRF, VRFHASHER, VRFPARAMS> VRFStakeTable<VRF, VRFHASHER, VRFPARAMS>
where
    VRF: Vrf<VRFHASHER, VRFPARAMS>,
    VRFPARAMS: Bls12Parameters,
    <VRFPARAMS as Bls12Parameters>::G1Parameters: SWHashToGroup,
    VRF::PublicKey: Clone,
{
    pub fn get_stake<SIGSCHEME>(&self, pk: &VRFPubKey<SIGSCHEME>) -> Option<u64>
    where
        SIGSCHEME: SignatureScheme<
            VerificationKey = VRF::PublicKey,
            PublicParameter = (),
            MessageUnit = u8,
        >,
        SIGSCHEME::VerificationKey: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
        SIGSCHEME::SigningKey: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
        SIGSCHEME::Signature: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
    {
        let encoded = pk.to_bytes();
        self.mapping.get(&encoded).map(|val| val.get())
    }
}

// struct Orderable<T> {
//     pub value: T,
//     serialized: Vec<u8>,
// }
//
// impl<T: serde::Serialize> serde::Serialize for Orderable<T> {
// }
//
//
// impl<T: serde::Serialize> Orderable<T> {
//     pub fn new(t: T) -> Self {
//         let bytes = bincode_opts().serialize(&t).unwrap();
//         Self {
//             value: t,
//             serialized: bytes
//         }
//     }
// }
//
// impl<T> Ord for Orderable<T> {
//     fn cmp(&self, other: &Self) -> std::cmp::Ordering {
//         self.serialized.cmp(&other.serialized)
//     }
// }
// impl<T> PartialOrd for Orderable<T> {
//     fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
//         self.serialized.partial_cmp(&other.serialized)
//     }
// }
//
// impl<T> Eq for Orderable<T> {
// }
//
// impl<T> PartialEq for Orderable<T> {
//     fn eq(&self, other: &Self) -> bool {
//         self.serialized == other.serialized
//     }
// }
//

// impl std::cmp::PartialOrd for Orderable<T> {}
// impl std::cmp::Ord for Orderable<T> {}

/// TODO this may not be correct for KEY

pub struct VrfImpl<STATE, SIGSCHEME, VRF, VRFHASHER, VRFPARAMS>
where
    VRF: Vrf<VRFHASHER, VRFPARAMS> + Sync + Send,
{
    stake_table: VRFStakeTable<VRF, VRFHASHER, VRFPARAMS>,
    proof_parameters: VRF::PublicParameter,
    prng: std::sync::Arc<std::sync::Mutex<rand_chacha::ChaChaRng>>,
    sortition_parameter: u64,
    // TODO (fst2) accessor to stake table
    // stake_table:
    _pd_0: PhantomData<VRFHASHER>,
    _pd_1: PhantomData<VRFPARAMS>,
    _pd_2: PhantomData<STATE>,
    _pd_3: PhantomData<VRF>,
    _pd_4: PhantomData<SIGSCHEME>,
}
impl<STATE, SIGSCHEME, VRF, VRFHASHER, VRFPARAMS> Clone
    for VrfImpl<STATE, SIGSCHEME, VRF, VRFHASHER, VRFPARAMS>
where
    VRF: Vrf<VRFHASHER, VRFPARAMS, PublicParameter = ()> + Sync + Send,
{
    fn clone(&self) -> Self {
        Self {
            stake_table: self.stake_table.clone(),
            proof_parameters: (),
            prng: self.prng.clone(),
            sortition_parameter: self.sortition_parameter,
            _pd_0: PhantomData,
            _pd_1: PhantomData,
            _pd_2: PhantomData,
            _pd_3: PhantomData,
            _pd_4: PhantomData,
        }
    }
}

/// TODO doc me
pub fn get_total_stake() {}

/// TODO doc me
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VRFVoteToken<VRF: Vrf<VRFHASHER, VRFPARAMS>, VRFHASHER, VRFPARAMS> {
    /// The public key assocaited with this token
    pub pub_key: VRF::PublicKey,
    /// The list of signatures
    pub proof: VRF::Proof,
    /// The number of signatures that are valid
    /// TODO (ct) this should be the sorition outbput
    pub count: u64,
}

impl<VRF, VRFHASHER, VRFPARAMS> VoteToken for VRFVoteToken<VRF, VRFHASHER, VRFPARAMS>
where
    VRF: Vrf<VRFHASHER, VRFPARAMS>,
{
    fn vote_count(&self) -> u64 {
        self.count
    }
}

// KEY is VRFPubKey
impl<VRFHASHER, VRFPARAMS, VRF, SIGSCHEME, TIME, STATE> Election<VRFPubKey<SIGSCHEME>, TIME>
    for VrfImpl<STATE, SIGSCHEME, VRF, VRFHASHER, VRFPARAMS>
where
    SIGSCHEME: SignatureScheme<PublicParameter = (), MessageUnit = u8> + Sync + Send,
    SIGSCHEME::VerificationKey: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
    SIGSCHEME::SigningKey: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
    SIGSCHEME::Signature: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
    VRF: Vrf<
            VRFHASHER,
            VRFPARAMS,
            Input = [u8; 32],
            Output = [u8; 32],
            PublicKey = SIGSCHEME::VerificationKey,
            SecretKey = SIGSCHEME::SigningKey,
        > + Sync
        + Send,
    VRF::Proof: Clone + Sync + Send + Serialize + for<'a> Deserialize<'a>,
    VRF::PublicParameter: Sync + Send,
    VRFHASHER: Clone + Sync + Send,
    VRFPARAMS: Sync + Send,
    TIME: ConsensusTime,
    STATE: State,
    VRFPARAMS: Bls12Parameters,
    <VRFPARAMS as Bls12Parameters>::G1Parameters: SWHashToGroup,
{
    // pubkey -> unit of stake
    type StakeTable = VRFStakeTable<VRF, VRFHASHER, VRFPARAMS>;

    type StateType = STATE;

    // TODO generics in terms of vrf trait output(s)
    // represents a vote on a proposal
    type VoteTokenType = VRFVoteToken<VRF, VRFHASHER, VRFPARAMS>;

    // FIXED STAKE
    // just return the state
    fn get_stake_table(
        &self,
        _view_number: hotshot_types::data::ViewNumber,
        _state: &Self::StateType,
    ) -> Self::StakeTable {
        self.stake_table.clone()
    }

    fn get_leader(&self, view_number: hotshot_types::data::ViewNumber) -> VRFPubKey<SIGSCHEME> {
        // TODO fst2 (ct) this is round robin, we should make this dependent on
        // the VRF + some source of randomness

        // TODO for now do by stake table of how much stake each
        // participant has
        let mapping = &self.stake_table.mapping;
        let index = ((*view_number) as usize) % mapping.len();
        let encoded = mapping.keys().nth(index).unwrap();
        SignatureKey::from_bytes(encoded).unwrap()
    }

    // what this is doing:
    // -
    fn make_vote_token(
        // TODO see if we can make this take &mut self
        // because we're using a mutable prng
        &self,
        view_number: hotshot_types::data::ViewNumber,
        private_key: &(SIGSCHEME::SigningKey, SIGSCHEME::VerificationKey),
        // TODO (ct) this should be replaced with something else...
        next_state: commit::Commitment<hotshot_types::data::Leaf<Self::StateType>>,
    ) -> Result<Option<Self::VoteTokenType>, hotshot_types::traits::election::ElectionError> {
        let pub_key = VRFPubKey::<SIGSCHEME>::from_native(private_key.1.clone());
        let replicas_stake = match self.stake_table.get_stake(&pub_key) {
            Some(val) => val,
            None => return Ok(None),
        };
        let view_seed = generate_view_seed(view_number, next_state);
        // TODO (ct) this can fail, return Result::Err
        let proof = VRF::prove(
            &self.proof_parameters,
            &private_key.0.clone(),
            &view_seed,
            &mut *self.prng.lock().unwrap()
        ).unwrap();

        // TODO (ct) this can fail, return result::err
        let hash = VRF::evaluate(&self.proof_parameters, &proof).unwrap();

        // TODO (ct) this should invoke the get_stake_table function
        let total_stake = self.stake_table.total_stake;

        // TODO (jr) this error handling is NOTGOOD
        let selected_stake = find_bin_idx(replicas_stake, total_stake, SORTITION_PARAMETER, &hash);
        match selected_stake {
            Some(count) => {
                // TODO (ct) this can fail, return Result::Err
                let proof = VRF::prove(
                    &self.proof_parameters,
                    &private_key.0,
                    &<[u8; 32]>::from(next_state),
                    &mut *self.prng.lock().unwrap(),
                )
                .unwrap();

                Ok(Some(VRFVoteToken {
                    pub_key: private_key.1.clone(),
                    proof,
                    count,
                }))
            }
            None => Ok(None),
        }
    }

    fn validate_vote_token(
        &self,
        view_number: hotshot_types::data::ViewNumber,
        pub_key: VRFPubKey<SIGSCHEME>,
        token: Checked<Self::VoteTokenType>,
        next_state: commit::Commitment<hotshot_types::data::Leaf<Self::StateType>>,
    ) -> Result<Checked<Self::VoteTokenType>, hotshot_types::traits::election::ElectionError> {
        match token {
            Checked::Unchecked(token) => {
                let stake : Option<u64> = self.stake_table.get_stake(&pub_key);
                if let Some(stake) = stake {
                    if let Ok(true) = VRF::verify(&self.proof_parameters, &token.proof, &pub_key.pk, &<[u8; 32]>::from(next_state)) {
                        if let Ok(seed) = VRF::evaluate(&self.proof_parameters, &token.proof) {
                            let total_stake = self.stake_table.total_stake;
                            if let Some(true) = check_bin_idx(token.count, stake, total_stake, SORTITION_PARAMETER, &seed) {
                                Ok(Checked::Valid(token))
                            } else {
                                Ok(Checked::Inval(token))
                            }
                        }
                        else {
                            Ok(Checked::Inval(token))
                        }
                    } else {
                        Ok(Checked::Inval(token))
                    }
                } else {
                    // TODO better error
                    Err(ElectionError::StubError)
                }
            }
            already_checked => Ok(already_checked),
        }
    }
}

// checks that the expected aomunt of stake matches the VRF output
// TODO this can be optimized most likely
fn check_bin_idx(expected_amount_of_stake: u64, replicas_stake: u64, total_stake: u64, sortition_parameter: u64, unnormalized_seed: &[u8; 32]) -> Option<bool> {
    let bin_idx = find_bin_idx(replicas_stake, total_stake, sortition_parameter, unnormalized_seed);
    bin_idx.map(|idx| idx == expected_amount_of_stake)
}

/// generates the seed from algorand paper
/// baseed on `next_state` commitment as of now, but in the future will be other things
/// this is a stop-gap
fn generate_view_seed<STATE: State>(_view_number: ViewNumber, next_state: commit::Commitment<hotshot_types::data::Leaf<STATE>>) -> [u8; 32] {
    <[u8; 32]>::from(next_state)
}

// Calculates B(j; w; p) where B means bernoulli distribution.
// That is: run w trials, with p probability of success for each trial, and return the probability
// of j successes.
// p = tau / W, where tau is the sortition parameter (controlling committee size)
// this is the only usage of W and tau
//
// Translation:
// stake_attempt: our guess at what the stake might be. This is j
// replicas_stake: the units of stake owned by the replica. This is w
// total_stake: the units of stake owned in total. This is W
// sorition_parameter: the parameter controlling the committee size. This is tau
//
// TODO (ct) better error handling
// returns none if one of our calculations fails
fn calculate_threshold(stake_attempt: u32, replicas_stake: u64, total_stake: u64, sortition_parameter: u64) -> Option<Ratio<BigUint>> {
    // TODO (ct) better error handling
    if stake_attempt as u64 > replicas_stake {
        panic!("j is larger than amount of stake we are allowed");
    }


    let sortition_parameter_big : BigUint = BigUint::from_u64(sortition_parameter)?;
    let total_stake_big : BigUint = BigUint::from_u64(sortition_parameter)?;

    // this is the p parameter for the bernoulli distribution
    let p = Ratio::new(sortition_parameter_big, total_stake_big);

    // number of tails in bernoulli
    let failed_num = replicas_stake - (stake_attempt as u64);

    let num_permutations = factorial(replicas_stake) / (factorial(stake_attempt as u64) * factorial(failed_num));
    let num_permutations = Ratio::new(num_permutations, BigUint::from_u8(1)?);

    let one = Ratio::new(BigUint::from_u8(1)?, BigUint::from_u8(1)?);

    let result = num_permutations * (p.pow(stake_attempt as i32) * (one - p).pow(failed_num as i32));

    Some(result)
}

// calculated i! as a biguint
fn factorial(mut i: u64) -> BigUint {
    let mut result = BigUint::from_usize(1).unwrap();
    while i > 0 {
        result *= i;
        i -= 1;
    }
    result
}

/// find the amount of stake we rolled.
/// NOTE: in the future this requires a view numb
fn find_bin_idx(replicas_stake: u64, total_stake: u64, sortition_parameter: u64, unnormalized_seed: &[u8; 32]) -> Option<u64> {
    let unnormalized_seed = BigUint::from_bytes_le(unnormalized_seed);
    let normalized_seed = Ratio::new(unnormalized_seed, BigUint::from_u8(1)?.pow(256));
    let mut j = 0;
    // left_threshold corresponds to the sum of all bernoulli distributions
    // from i in 0 to j: B(i; replicas_stake; p). Where p is calculated later and corresponds to
    // algorands paper
    let mut left_threshold = Ratio::new(BigUint::from_u8(0)?, BigUint::from_u8(1)?);
    loop {
        let bin_val = calculate_threshold(j + 1, replicas_stake, total_stake, sortition_parameter)?;
        // corresponds to right range from apper
        let right_threshold = left_threshold + bin_val;
        // from i in 0 to j + 1: B(i; replicas_stake; p)
        if normalized_seed < right_threshold {
            return Some(j as u64);
        } else {
            left_threshold = right_threshold;
            j += 1;
        }
    }
}

impl<STATE, SIGSCHEME, VRF, VRFHASHER, VRFPARAMS>
    VrfImpl<STATE, SIGSCHEME, VRF, VRFHASHER, VRFPARAMS>
where
    SIGSCHEME: SignatureScheme<PublicParameter = (), MessageUnit = u8> + Sync + Send,
    SIGSCHEME::VerificationKey: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
    SIGSCHEME::SigningKey: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
    SIGSCHEME::Signature: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
    VRF: Vrf<
            VRFHASHER,
            VRFPARAMS,
            PublicParameter = (),
            Input = [u8; 32],
            Output = [u8; 32],
            PublicKey = SIGSCHEME::VerificationKey,
            SecretKey = SIGSCHEME::SigningKey,
        > + Sync
        + Send,
    VRF::Proof: Clone + Sync + Send + Serialize + for<'a> Deserialize<'a>,
    VRF::PublicParameter: Sync + Send,
    VRFHASHER: Clone + Sync + Send,
    VRFPARAMS: Sync + Send,
    STATE: State,
    VRFPARAMS: Bls12Parameters,
    <VRFPARAMS as Bls12Parameters>::G1Parameters: SWHashToGroup,
{
    pub fn with_initial_stake(known_nodes: Vec<VRFPubKey<SIGSCHEME>>, sortition_parameter: u64) -> Self {
        VrfImpl {
            stake_table: VRFStakeTable {
                mapping: known_nodes
                    .iter()
                    .map(|k| (k.to_bytes(), NonZeroU64::new(100u64).unwrap()))
                    .collect(),
                total_stake: known_nodes.len() as u64 * 100,
                _pd_0: PhantomData,
                _pd_1: PhantomData,
                _pd_2: PhantomData,
            },
            proof_parameters: (),
            sortition_parameter,
            // #[serde(ignore)]
            prng: Arc::new(Mutex::new(ChaChaRng::from_seed(Default::default()))),
            // TODO (fst2) accessor to stake table
            // stake_table:
            _pd_0: PhantomData,
            _pd_1: PhantomData,
            _pd_2: PhantomData,
            _pd_3: PhantomData,
            _pd_4: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blake3::Hasher;
    use commit::{RawCommitmentBuilder, Commitment};
    use hotshot_types::{traits::state::dummy::DummyState, data::{ViewNumber, fake_commitment, Leaf, random_commitment}};
    use hotshot_utils::hack::nll_todo;
    use jf_primitives::vrf::blsvrf::BLSVRFScheme;
    use rand::RngCore;
    use std::collections::BTreeMap;
    use ark_std::{rand::prelude::StdRng, test_rng};
    use sha2::Sha256;
    use ark_bls12_381::Parameters as Param381;

    pub fn gen_vrf_impl(num_nodes: usize) -> (VrfImpl<DummyState, BLSSignatureScheme<Param381>, BLSVRFScheme<Param381>, Hasher, Param381>, Vec<(jf_primitives::signatures::bls::BLSSignKey<Param381>, jf_primitives::signatures::bls::BLSVerKey<Param381>)>) {
        let mut known_nodes = Vec::new();
        let mut keys = Vec::new();
        for i in 0..num_nodes {
            let rng = &mut test_rng();
            let parameters = BLSSignatureScheme::<Param381>::param_gen(Some(rng)).unwrap();
            let (sk, pk) = BLSSignatureScheme::<Param381>::key_gen(&parameters, rng).unwrap();
            keys.push((sk.clone(), pk.clone()));
            known_nodes.push(VRFPubKey::from_native(pk.clone()));
        }
        let stake_table = VrfImpl::with_initial_stake(known_nodes, SORTITION_PARAMETER);
        (stake_table, keys)
    }

    #[test]
    pub fn test_sorition(){
        let (vrf_impl, keys) = gen_vrf_impl(10);
        let rounds = 1;

        for i in 0..rounds {
            let next_state_commitment : Commitment<Leaf<DummyState>> = random_commitment();
            for (pk, sk) in keys.iter() {
                let token =
                    <crate::traits::election::vrf::VrfImpl<hotshot_types::traits::block_contents::dummy::DummyState, jf_primitives::signatures::BLSSignatureScheme<ark_bls12_381::Parameters>, BLSVRFScheme<ark_bls12_381::Parameters>, blake3::Hasher, ark_bls12_381::Parameters> as hotshot_types::traits::election::Election<crate::traits::election::vrf::VRFPubKey<jf_primitives::signatures::BLSSignatureScheme<ark_bls12_381::Parameters>>, ViewNumber>>::make_vote_token(&vrf_impl, ViewNumber::new(i), &(pk.clone(), sk.clone()), next_state_commitment).unwrap().unwrap();
                // vrf_impl.validate_vote_token(i, pk, token, next_state_commitment)
                nll_todo()
            }
        }



        let sortition_parameter = 10;

        let unnormalized_seed : [u8; 32] = nll_todo();

        let message = [0u8; 32];
        let message_bad = [1u8; 32];
        // find_bin_idx();
    }

    // macro_rules! test_bls_vrf {
    //     ($curve_param:tt) => {
    //         let message = [0u8; 32];
    //         let message_bad = [1u8; 32];
    //         sign_and_verify::<BLSVRFScheme<$curve_param>, Sha256, _>(&message);
    //         failed_verification::<BLSVRFScheme<$curve_param>, Sha256, _>(&message, &message_bad);
    //     };
    // }
    //
    // fn test_bls_vrf() {
    //     test_bls_vrf!(Param377);
    //     test_bls_vrf!(Param381);
    // }
    //
    // pub(crate) fn sign_and_verify<S: SignatureScheme>(message: &[S::MessageUnit]) {
    //     let rng = &mut test_rng();
    //     let parameters = S::param_gen(Some(rng)).unwrap();
    //     let (sk, pk) = S::key_gen(&parameters, rng).unwrap();
    //     let sig = S::sign(&parameters, &sk, &message, rng).unwrap();
    //     assert!(S::verify(&parameters, &pk, &message, &sig).is_ok());
    //
    //     let parameters = S::param_gen::<StdRng>(None).unwrap();
    //     let (sk, pk) = S::key_gen(&parameters, rng).unwrap();
    //     let sig = S::sign(&parameters, &sk, &message, rng).unwrap();
    //     assert!(S::verify(&parameters, &pk, &message, &sig).is_ok());
    // }

    // pub(crate) fn failed_verification<S: SignatureScheme>(
    //     message: &[S::MessageUnit],
    //     bad_message: &[S::MessageUnit],
    // ) {
    //     let rng = &mut test_rng();
    //     let parameters = S::param_gen(Some(rng)).unwrap();
    //     let (sk, pk) = S::key_gen(&parameters, rng).unwrap();
    //     let sig = S::sign(&parameters, &sk, message, rng).unwrap();
    //     assert!(!S::verify(&parameters, &pk, bad_message, &sig).is_ok());
    // }

}
