use ark_ec::bls12::Bls12Parameters;
use bincode::Options;
use hotshot_types::{traits::{
    election::{Checked, Election, ElectionError, VoteToken, ElectionConfig},
    signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey, TestableSignatureKey},
    State,
}, data::ViewNumber};
use hotshot_utils::bincode::bincode_opts;
use jf_primitives::{
    hash_to_group::SWHashToGroup,
    signatures::SignatureScheme,
    vrf::Vrf,
};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use serde::{
    de::{self},
    Deserialize, Serialize,
};
use tracing::{instrument, error, info};
use std::{
    collections::BTreeMap,
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
    num::NonZeroU64,
    sync::{Arc, Mutex},
};

use num::{rational::Ratio, BigUint, ToPrimitive};

// TODO wrong palce for this
/// the sortition committee size parameter
pub const SORTITION_PARAMETER: u64 = 100;

// TODO abstraction this function's impl into a trait
// TODO do we necessariy want the units of stake to be a u64? or generics
/// The stake table for VRFs
#[derive(Serialize, Deserialize, Debug)]
pub struct VRFStakeTable<VRF, VRFHASHER, VRFPARAMS> {
    /// the mapping of id -> stake
    mapping: BTreeMap<EncodedPublicKey, NonZeroU64>,
    /// total stake present
    total_stake: u64,
    /// PhantomData for VRF
    _pd_0: PhantomData<VRF>,
    /// PhantomData for VRFHASEHR
    _pd_1: PhantomData<VRFHASHER>,
    /// PhantomData for VRFPARAMS
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

/// type wrapper for VRF's public key
#[derive(Deserialize, Serialize)]
pub struct VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme,
    SIGSCHEME::VerificationKey: Clone,
{
    /// the public key
    pk: SIGSCHEME::VerificationKey,
    /// phantom data
    _pd_0: PhantomData<SIGSCHEME::SigningKey>,
}

impl<SIGSCHEME> TestableSignatureKey for VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme<PublicParameter = (), MessageUnit = u8> + Sync + Send,
    SIGSCHEME::VerificationKey: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
    SIGSCHEME::SigningKey: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
    SIGSCHEME::Signature: Clone + Serialize + for<'a> Deserialize<'a> + Sync + Send,
{
    fn generate_test_key(id: u64) -> Self::PrivateKey {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&id.to_le_bytes());
        let new_seed = *hasher.finalize().as_bytes();
        let mut prng = rand::rngs::StdRng::from_seed(new_seed);
        // TODO we should make this more general/use different parameters
        #[allow(clippy::let_unit_value)]
        let parameters = SIGSCHEME::param_gen(Some(&mut prng)).unwrap();
        SIGSCHEME::key_gen(&parameters, &mut prng).unwrap()
    }
}

impl<SIGSCHEME> VRFPubKey<SIGSCHEME>
where
    SIGSCHEME: SignatureScheme,
    SIGSCHEME::VerificationKey: Clone,
{
    /// wrap the public key
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
    // TODO this is wrong.
    fn generated_from_seed_indexed(_seed: [u8; 32], _index: u64) -> (Self, Self::PrivateKey) {
        let mut prng = rand::thread_rng();

        // TODO we should make this more general/use different parameters
        #[allow(clippy::let_unit_value)]
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
    /// get total stake
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
    /// get total stake
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

/// the vrf implementation
pub struct VrfImpl<STATE, SIGSCHEME, VRF, VRFHASHER, VRFPARAMS>
where
    VRF: Vrf<VRFHASHER, VRFPARAMS> + Sync + Send,
{
    /// the stake table
    stake_table: VRFStakeTable<VRF, VRFHASHER, VRFPARAMS>,
    /// the proof params
    proof_parameters: VRF::PublicParameter,
    /// the rng
    prng: std::sync::Arc<std::sync::Mutex<rand_chacha::ChaChaRng>>,
    /// the committee parameter
    sortition_parameter: u64,
    // TODO (fst2) accessor to stake table
    // stake_table:
    /// phantom data
    _pd_0: PhantomData<VRFHASHER>,
    /// phantom data
    _pd_1: PhantomData<VRFPARAMS>,
    /// phantom data
    _pd_2: PhantomData<STATE>,
    /// phantom data
    _pd_3: PhantomData<VRF>,
    /// phantom data
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
#[derive(Serialize, Deserialize, Debug)]
pub struct VRFVoteToken<VRF: Vrf<VRFHASHER, VRFPARAMS>, VRFHASHER, VRFPARAMS>
{
    /// The public key assocaited with this token
    pub pub_key: VRF::PublicKey,
    /// The list of signatures
    pub proof: VRF::Proof,
    /// The number of signatures that are valid
    /// TODO (ct) this should be the sorition outbput
    pub count: u64,
}

impl<VRF: Vrf<VRFHASHER, VRFPARAMS>, VRFHASHER, VRFPARAMS> Clone for VRFVoteToken<VRF, VRFHASHER, VRFPARAMS>
where VRF::PublicKey: Clone, VRF::Proof: Clone
{
    fn clone(&self) -> Self {
        Self {
            pub_key: self.pub_key.clone(),
            proof: self.proof.clone(),
            count: self.count
        }
    }
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
impl<VRFHASHER, VRFPARAMS, VRF, SIGSCHEME, STATE> Election<VRFPubKey<SIGSCHEME>, <STATE as State>::Time>
    for VrfImpl<STATE, SIGSCHEME, VRF, VRFHASHER, VRFPARAMS>
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
    VRFPARAMS: Sync + Send + Bls12Parameters,
    STATE: State,
    <VRFPARAMS as Bls12Parameters>::G1Parameters: SWHashToGroup,
{
    // pubkey -> unit of stake
    type StakeTable = VRFStakeTable<VRF, VRFHASHER, VRFPARAMS>;

    type StateType = STATE;

    // TODO generics in terms of vrf trait output(s)
    // represents a vote on a proposal
    type VoteTokenType = VRFVoteToken<VRF, VRFHASHER, VRFPARAMS>;

    type ElectionConfigType = VRFStakeTableConfig;

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
        _view_number: hotshot_types::data::ViewNumber,
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

    fn create_election(keys: Vec<VRFPubKey<SIGSCHEME>>, config: Self::ElectionConfigType) -> Self {
        VrfImpl::with_initial_stake(keys, &config)
    }

    fn default_election_config(num_nodes: u64) -> Self::ElectionConfigType {
        let mut stake = Vec::new();
        let units_of_stake_per_node = NonZeroU64::new(100).unwrap();
        for _ in 0..num_nodes {
            stake.push(units_of_stake_per_node);

        }
        VRFStakeTableConfig {
            sortition_parameter: SORTITION_PARAMETER,
            distribution: stake,
        }
    }

    fn get_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((self.sortition_parameter * 2) / 3) + 1).unwrap()
    }
}

/// checks that the expected aomunt of stake matches the VRF output
/// TODO this can be optimized most likely
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
//
// TODO keep data around from last iteration so less calculation is needed
// TODO test this "correct/simple" implementation against any optimized version
#[instrument]
fn calculate_threshold(stake_attempt: u32, replicas_stake: u64, total_stake: u64, sortition_parameter: u64) -> Option<Ratio<BigUint>> {
    let stake_attempt = u64::from(stake_attempt);
    tracing::info!("Running calculate threshold");
    // TODO (ct) better error handling
    if stake_attempt as u64 > replicas_stake {
        error!("j is larger than amount of stake we are allowed");
        return None;
    }


    let sortition_parameter_big : BigUint = BigUint::from(sortition_parameter);
    let total_stake_big : BigUint = BigUint::from(total_stake);
    let one_big = BigUint::from(1_u32);

    // this is the p parameter for the bernoulli distribution
    let p = Ratio::new(sortition_parameter_big, total_stake_big);

    assert!(p.numer() < p.denom());

    info!("p is {p:?}");

    // number of tails in bernoulli
    let failed_num = replicas_stake - stake_attempt;

    // TODO cancel things out (avoid calculating factorial)
    // TODO can just do division
    let num_permutations = Ratio::new(factorial(replicas_stake), factorial(stake_attempt) * factorial(failed_num));

    info!("num permutations is {num_permutations:?}, failed_num is {failed_num:?}");

    let one = Ratio::from_integer(one_big);

    // TODO can keep results from last try
    let result = num_permutations * (p.pow(i32::try_from(stake_attempt).ok()?) * (one - p).pow(i32::try_from(failed_num).ok()?));

    assert!(result.numer() < result.denom());

    info!("result is is {result:?}");

    Some(result)
}

/// compute i! as a biguint
fn factorial(mut i: u64) -> BigUint {
    if i == 0 { return BigUint::from(1u32) }

    let mut result = BigUint::from(1u32);
    while i > 0 {
        result *= i;
        i -= 1;
    }
    result
}

/// find the amount of stake we rolled.
/// NOTE: in the future this requires a view numb
#[instrument]
fn find_bin_idx(replicas_stake: u64, total_stake: u64, sortition_parameter: u64, unnormalized_seed: &[u8; 32]) -> Option<u64> {
    let unnormalized_seed = BigUint::from_bytes_le(unnormalized_seed);
    let normalized_seed = Ratio::new(unnormalized_seed, BigUint::from(2_u32).pow(256));
    assert!(normalized_seed.numer() < normalized_seed.denom());
    let mut j = 0;

    // [j, j+1)
    // [cdf(j),cdf(j+1))

    // left_threshold corresponds to the sum of all bernoulli distributions
    // from i in 0 to j: B(i; replicas_stake; p). Where p is calculated later and corresponds to
    // algorands paper
    let mut left_threshold = Ratio::from_integer(BigUint::from(0u32));
    loop {
        assert!(left_threshold.numer() < left_threshold.denom());
        let bin_val = calculate_threshold(j + 1, replicas_stake, total_stake, sortition_parameter)?;
        // corresponds to right range from apper
        let right_threshold = left_threshold + bin_val.clone();

        {
            let right_threshold_float = ToPrimitive::to_f64(&right_threshold.clone());
            let bin_val_float = ToPrimitive::to_f64(&bin_val.clone());
            let normalized_seed_float = ToPrimitive::to_f64(&normalized_seed.clone());
            info!("rightthreshold: {right_threshold_float:?}, bin: {bin_val_float:?}, seed: {normalized_seed_float:?}");
        }

        // from i in 0 to j + 1: B(i; replicas_stake; p)
        if normalized_seed < right_threshold {
            return Some(u64::from(j));
        }
        left_threshold = right_threshold;
        j += 1;
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
    VRFPARAMS: Sync + Send + Bls12Parameters,
    STATE: State,
    <VRFPARAMS as Bls12Parameters>::G1Parameters: SWHashToGroup,
{
    /// create stake table with this initial stake
    /// # Panics
    /// TODO
    pub fn with_initial_stake(known_nodes: Vec<VRFPubKey<SIGSCHEME>>, config: &VRFStakeTableConfig) -> Self {
        assert_eq!(known_nodes.iter().len(), config.distribution.len());
        let key_with_stake = known_nodes.into_iter().map(|x| x.to_bytes()).zip(config.distribution.clone()).collect();
        error!("stake table: {:?}", key_with_stake);
        VrfImpl {
            stake_table: {
                let st=VRFStakeTable {
                    mapping: key_with_stake,
                    total_stake: config.distribution.iter().map(|x| x.get()).sum(),
                    _pd_0: PhantomData,
                    _pd_1: PhantomData,
                    _pd_2: PhantomData,
                };
                st
            },
            proof_parameters: (),
            prng: Arc::new(Mutex::new(ChaChaRng::from_seed(Default::default()))),
            // TODO (fst2) accessor to stake table
            // stake_table:
            _pd_0: PhantomData,
            _pd_1: PhantomData,
            _pd_2: PhantomData,
            _pd_3: PhantomData,
            _pd_4: PhantomData,
            sortition_parameter: config.sortition_parameter,
        }
    }
}

/// configuration specifying the stake table
#[derive(Default, Clone, Serialize, Deserialize, core::fmt::Debug)]
pub struct VRFStakeTableConfig {
    /// the committee size parameter
    pub sortition_parameter: u64,
    /// the ordered distribution of stake across nodes
    pub distribution: Vec<NonZeroU64>
}

impl ElectionConfig for VRFStakeTableConfig {
}

#[cfg(test)]
mod tests {
    use super::*;
    use blake3::Hasher;
    use commit::Commitment;
    use hotshot_types::{traits::state::dummy::DummyState, data::{ViewNumber, Leaf, random_commitment}};
    use hotshot_utils::test_util::setup_logging;
    use jf_primitives::{vrf::blsvrf::BLSVRFScheme, signatures::BLSSignatureScheme};
    use ark_std::test_rng;
    use ark_bls12_381::Parameters as Param381;

    pub fn gen_vrf_impl(num_nodes: usize) -> (VrfImpl<DummyState, BLSSignatureScheme<Param381>, BLSVRFScheme<Param381>, Hasher, Param381>, Vec<(jf_primitives::signatures::bls::BLSSignKey<Param381>, jf_primitives::signatures::bls::BLSVerKey<Param381>)>) {
        let mut known_nodes = Vec::new();
        let mut keys = Vec::new();
        let rng = &mut test_rng();
        let mut stake_distribution = Vec::new();
        let stake_per_node = NonZeroU64::new(100).unwrap();
        for _i in 0..num_nodes {
            // TODO we should make this more general/use different parameters
            #[allow(clippy::let_unit_value)]
            let parameters = BLSSignatureScheme::<Param381>::param_gen(Some(rng)).unwrap();
            let (sk, pk) = BLSSignatureScheme::<Param381>::key_gen(&parameters, rng).unwrap();
            keys.push((sk.clone(), pk.clone()));
            known_nodes.push(VRFPubKey::from_native(pk.clone()));
            stake_distribution.push(stake_per_node);
        }
        let stake_table = VrfImpl::with_initial_stake(known_nodes, &VRFStakeTableConfig {
            sortition_parameter: SORTITION_PARAMETER,
            distribution: stake_distribution
        });
        (stake_table, keys)
    }

    pub fn check_if_valid<T>(token: &Checked<T>) -> bool {
        match token {
            Checked::Valid(_) => true,
            Checked::Inval(_) | Checked::Unchecked(_) => false,
        }

    }

    #[test]
    pub fn test_sortition(){
        setup_logging();
        let (vrf_impl, keys) = gen_vrf_impl(10);
        let views = 100;

        for view in 0..views {
            let next_state_commitment : Commitment<Leaf<DummyState>> = random_commitment();
            for (node_idx, (sk, pk)) in keys.iter().enumerate() {
                let token = vrf_impl.make_vote_token(ViewNumber::new(view), &(sk.clone(), pk.clone()), next_state_commitment).unwrap().unwrap();
                let count = token.count;
                let result = vrf_impl.validate_vote_token(ViewNumber::new(view), VRFPubKey::from_native(pk.clone()), Checked::Unchecked(token), next_state_commitment).unwrap();
                let result_is_valid = check_if_valid(&result);
                error!("view {view:?}, node_idx {node_idx:?}, stake {count:?} ");
                assert!(result_is_valid);
            }
        }
    }

    #[test]
    pub fn test_factorial(){
        assert_eq!(factorial(0), BigUint::from(1u32));
        assert_eq!(factorial(1), BigUint::from(1u32));
        assert_eq!(factorial(2), BigUint::from(2u32));
        assert_eq!(factorial(3), BigUint::from(6u32));
        assert_eq!(factorial(4), BigUint::from(24u32));
        assert_eq!(factorial(5), BigUint::from(120u32));
    }

    // TODO add failure case
}
