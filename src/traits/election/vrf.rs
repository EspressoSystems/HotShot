use ark_ec::bls12::Bls12Parameters;
use bincode::Options;
use hotshot_types::{
    data::ViewNumber,
    traits::{
        election::{Checked, Election, ElectionConfig, ElectionError, VoteToken},
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey, TestableSignatureKey},
        State,
    },
};
use hotshot_utils::bincode::bincode_opts;
use jf_primitives::{hash_to_group::SWHashToGroup, signatures::SignatureScheme, vrf::Vrf};
use rand::SeedableRng;
use rand_chacha::ChaChaRng;
use serde::{
    de::{self},
    Deserialize, Serialize,
};
use std::time::Instant;
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
    num::NonZeroU64,
    sync::{Arc, Mutex, MutexGuard},
};
use tracing::{error, info, instrument};

use num::{rational::Ratio, BigUint, ToPrimitive};

// TODO wrong palce for this
/// the sortition committee size parameter
pub const SORTITION_PARAMETER: u64 = 10000;

// TODO abstraction this function's impl into a trait
// TODO do we necessariy want the units of stake to be a u64? or generics
/// The stake table for VRFs
#[derive(Serialize, Deserialize, Debug)]
pub struct VRFStakeTable<VRF, VRFHASHER, VRFPARAMS> {
    /// the mapping of id -> stake
    mapping: BTreeMap<EncodedPublicKey, NonZeroU64>,
    /// total stake present
    total_stake: NonZeroU64,
    // NODES TODO REMOVE
    nodes: Vec<EncodedPublicKey>,
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
            nodes: self.nodes.clone(),
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
        f.debug_tuple("VRFPubKey")
            .field(&base64::encode(&self.to_bytes().0))
            .finish()
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
        let start = Instant::now();
        let x: Result<SIGSCHEME::Signature, _> = bincode_opts().deserialize(&signature.0);
        match x {
            Ok(s) => {
                // First hash the data into a constant sized digest
                let result = SIGSCHEME::verify(&(), &self.pk, data, &s).is_ok();
                println!("Duration for verifying signature is {:?}", Instant::now() - start);

                result
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
    fn generated_from_seed_indexed(_seed: [u8; 32], index: u64) -> (Self, Self::PrivateKey) {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&index.to_le_bytes());
        let new_seed = *hasher.finalize().as_bytes();
        let mut prng = rand::rngs::StdRng::from_seed(new_seed);
        // TODO we should make this more general/use different parameters
        #[allow(clippy::let_unit_value)]
        let parameters = SIGSCHEME::param_gen(Some(&mut prng)).unwrap();
        let (sk, pk) = SIGSCHEME::key_gen(&parameters, &mut prng).unwrap();
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
    pub fn get_all_stake(&self) -> NonZeroU64 {
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
    /// # Panics
    /// If converting non-zero stake into `NonZeroU64` fails
    pub fn get_stake<SIGSCHEME>(&self, pk: &VRFPubKey<SIGSCHEME>) -> Option<NonZeroU64>
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
        let stake = self.mapping.get(&encoded).map(|val| val.get());
        stake.and_then(NonZeroU64::new)
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
    sortition_parameter: NonZeroU64,
    /// Nodes
    // nodes: Vec<VRFPubKey<SIGSCHEME>>,

    /// pdf cache
    /// Why not async
    sortition_cache: std::sync::Arc<std::sync::Mutex<HashMap<BinomialQuery, Ratio<BigUint>>>>,

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
            sortition_cache: Arc::default(),
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
pub struct VRFVoteToken<VRF: Vrf<VRFHASHER, VRFPARAMS>, VRFHASHER, VRFPARAMS> {
    /// The public key assocaited with this token
    pub pub_key: VRF::PublicKey,
    /// The list of signatures
    pub proof: VRF::Proof,
    /// The number of signatures that are valid
    /// TODO (ct) this should be the sorition outbput
    pub count: NonZeroU64,
}

impl<VRF: Vrf<VRFHASHER, VRFPARAMS>, VRFHASHER, VRFPARAMS> Clone
    for VRFVoteToken<VRF, VRFHASHER, VRFPARAMS>
where
    VRF::PublicKey: Clone,
    VRF::Proof: Clone,
{
    fn clone(&self) -> Self {
        Self {
            pub_key: self.pub_key.clone(),
            proof: self.proof.clone(),
            count: self.count,
        }
    }
}

impl<VRF, VRFHASHER, VRFPARAMS> VoteToken for VRFVoteToken<VRF, VRFHASHER, VRFPARAMS>
where
    VRF: Vrf<VRFHASHER, VRFPARAMS>,
{
    fn vote_count(&self) -> NonZeroU64 {
        self.count
    }
}

// KEY is VRFPubKey
impl<VRFHASHER, VRFPARAMS, VRF, SIGSCHEME, STATE>
    Election<VRFPubKey<SIGSCHEME>, <STATE as State>::Time>
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
        // TODO change back to index
        // let encoded = mapping.keys().nth(index).unwrap();
        // SignatureKey::from_bytes(encoded).unwrap();

        // let index = (*view_number % self.nodes.len() as u64) as usize;
        let encoded = &self.stake_table.nodes[999]; 
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
            &mut *self.prng.lock().unwrap(),
        )
        .unwrap();

        // TODO (ct) this can fail, return result::err
        let hash = VRF::evaluate(&self.proof_parameters, &proof).unwrap();

        // TODO (ct) this should invoke the get_stake_table function
        let total_stake = self.stake_table.total_stake;

        // TODO (jr) this error handling is NOTGOOD
        let cache = self.sortition_cache.lock().unwrap();
        // error!("Cache is {:?}", std::mem::size_of::<Ratio<BigUint>>());
        let selected_stake = find_bin_idx(
            u64::from(replicas_stake),
            u64::from(total_stake),
            self.sortition_parameter.get(),
            &hash,
            cache,
        );
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
        let start = Instant::now();
        match token {
            Checked::Unchecked(token) => {
                let stake: Option<NonZeroU64> = self.stake_table.get_stake(&pub_key);
                if let Some(stake) = stake {
                    if let Ok(true) = VRF::verify(
                        &self.proof_parameters,
                        &token.proof,
                        &pub_key.pk,
                        &<[u8; 32]>::from(next_state),
                    ) {
                        // error!("Validating vote token");
                        if let Ok(seed) = VRF::evaluate(&self.proof_parameters, &token.proof) {
                            let total_stake = self.stake_table.total_stake;
                            if let Some(true) = check_bin_idx(
                                u64::from(token.count),
                                u64::from(stake),
                                u64::from(total_stake),
                                self.sortition_parameter.get(),
                                &seed,
                                self.sortition_cache.lock().unwrap(),
                            ) {
                                error!("Duration is {:?}", Instant::now() - start);
                                Ok(Checked::Valid(token))
                            } else {
                                Ok(Checked::Inval(token))
                            }
                        } else {
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
            sortition_parameter: NonZeroU64::new(SORTITION_PARAMETER).unwrap(),
            distribution: stake,
        }
    }

    fn get_threshold(&self) -> NonZeroU64 {
        NonZeroU64::new(((u64::from(self.sortition_parameter) * 2) / 3) + 1).unwrap()
    }
}

/// checks that the expected aomunt of stake matches the VRF output
/// TODO this can be optimized most likely
fn check_bin_idx(
    expected_amount_of_stake: u64,
    replicas_stake: u64,
    total_stake: u64,
    sortition_parameter: u64,
    unnormalized_seed: &[u8; 32],
    cache: MutexGuard<'_, HashMap<BinomialQuery, Ratio<BigUint>>>,
) -> Option<bool> {
    let bin_idx = find_bin_idx(
        replicas_stake,
        total_stake,
        sortition_parameter,
        unnormalized_seed,
        cache,
    );
    bin_idx.map(|idx| idx == NonZeroU64::new(expected_amount_of_stake).unwrap())
}

/// generates the seed from algorand paper
/// baseed on `next_state` commitment as of now, but in the future will be other things
/// this is a stop-gap
fn generate_view_seed<STATE: State>(
    _view_number: ViewNumber,
    next_state: commit::Commitment<hotshot_types::data::Leaf<STATE>>,
) -> [u8; 32] {
    <[u8; 32]>::from(next_state)
}

/// represents a binomial query made by sortition
/// `B(stake_attempt; replicas_stake; sortition_parameter / total_stake)`
#[derive(Hash, Eq, PartialEq, Clone, Debug)]
pub struct BinomialQuery {
    /// the number of heads
    stake_attempt: u64,
    /// the total number of coin flips
    replicas_stake: u64,
    /// the total amount of stake
    total_stake: u64,
    /// the sortition parameter
    sortition_parameter: u64,
}

impl BinomialQuery {
    /// get the committee parameter
    /// for this query
    pub fn get_p(&self) -> Ratio<BigUint> {
        let sortition_parameter_big: BigUint = BigUint::from(self.sortition_parameter);
        let total_stake_big: BigUint = BigUint::from(self.total_stake);
        Ratio::new(sortition_parameter_big, total_stake_big)
    }
}

#[instrument]
fn calculate_threshold_from_cache(
    previous_calculation: Option<(BinomialQuery, Ratio<BigUint>)>,
    query: BinomialQuery,
) -> Option<Ratio<BigUint>> {
    if let Some((previous_query, previous_result)) = previous_calculation {
        let expected_previous_query = BinomialQuery {
            stake_attempt: query.stake_attempt - 1,
            ..query
        };
        if previous_query == expected_previous_query {
            // error!("replica stake = {}, stake attempt = {}", query.replicas_stake, query.stake_attempt);
            let permutation = Ratio::new(
                BigUint::from(query.replicas_stake - query.stake_attempt + 1),
                BigUint::from(query.stake_attempt),
            );
            let p = query.get_p();
            assert!(p.numer() < p.denom());
            let reciprocal = Ratio::recip(&(Ratio::from_integer(BigUint::from(1_u32)) - p.clone()));
            let result = previous_result * p * reciprocal * permutation;
            assert!(result.numer() < result.denom());

            return Some(result);
        }
    }
    calculate_threshold(query)
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
// fn calculate_threshold(stake_attempt: u32, replicas_stake: u64, total_stake: u64, sortition_parameter: u64) -> Option<Ratio<BigUint>> {
fn calculate_threshold(query: BinomialQuery) -> Option<Ratio<BigUint>> {
    let stake_attempt = query.stake_attempt;
    tracing::info!("Running calculate threshold");
    // TODO (ct) better error handling
    if stake_attempt as u64 > query.replicas_stake {
        error!("j is larger than amount of stake we are allowed");
        return None;
    }

    let sortition_parameter_big: BigUint = BigUint::from(query.sortition_parameter);
    let total_stake_big: BigUint = BigUint::from(query.total_stake);
    let one_big = BigUint::from(1_u32);

    // this is the p parameter for the bernoulli distribution
    let p = Ratio::new(sortition_parameter_big, total_stake_big);

    assert!(p.numer() <= p.denom());

    info!("p is {p:?}");

    // number of tails in bernoulli
    let failed_num = query.replicas_stake - stake_attempt;

    // TODO cancel things out (avoid calculating factorial)
    // TODO can just do division
    let num_permutations = Ratio::new(
        factorial(query.replicas_stake),
        factorial(stake_attempt) * factorial(failed_num),
    );

    info!("num permutations is {num_permutations:?}, failed_num is {failed_num:?}");

    let one = Ratio::from_integer(one_big);

    // TODO can keep results from last try
    let result = num_permutations
        * (p.pow(i32::try_from(stake_attempt).ok()?)
            * (one - p).pow(i32::try_from(failed_num).ok()?));

    assert!(result.numer() < result.denom());

    info!("result is is {result:?}");

    Some(result)
}

/// compute i! as a biguint
fn factorial(mut i: u64) -> BigUint {
    if i == 0 {
        return BigUint::from(1u32);
    }

    let mut result = BigUint::from(1u32);
    while i > 0 {
        result *= i;
        i -= 1;
    }
    result
}

/// find the amount of stake we rolled.
/// NOTE: in the future this requires a view numb
/// Returns None if zero stake was rolled
#[instrument]
fn find_bin_idx(
    replicas_stake: u64,
    total_stake: u64,
    sortition_parameter: u64,
    unnormalized_seed: &[u8; 32],
    mut cache: MutexGuard<'_, HashMap<BinomialQuery, Ratio<BigUint>>>,
) -> Option<NonZeroU64> {
    let unnormalized_seed = BigUint::from_bytes_le(unnormalized_seed);
    let normalized_seed = Ratio::new(unnormalized_seed, BigUint::from(2_u32).pow(256));
    assert!(normalized_seed.numer() < normalized_seed.denom());
    let mut j: u64 = 0;

    // [j, j+1)
    // [cdf(j),cdf(j+1))

    // left_threshold corresponds to the sum of all bernoulli distributions
    // from i in 0 to j: B(i; replicas_stake; p). Where p is calculated later and corresponds to
    // algorands paper
    let mut left_threshold = Ratio::from_integer(BigUint::from(0u32));

    // // let cache = HashMap<BinomialQuery, Ratio<BigUint>>::new(); 
    // let mut cache: HashMap<BinomialQuery, Ratio<BigUint>> = HashMap::new(); 

    loop {
        // check cache

        // if cache miss, feed in with previous val from cache
        // that *probably* exists

        assert!(left_threshold.numer() < left_threshold.denom());
        let query = BinomialQuery {
            stake_attempt: j + 1,
            replicas_stake,
            total_stake,
            sortition_parameter,
        };

        let bin_val = {
            // we already computed this value
            if let Some(result) = cache.get(&query) {
                result.clone()
            } else {
                // we haven't computed this value, but maybe
                // we already computed the previous value

                let mut maybe_old_query = query.clone();
                maybe_old_query.stake_attempt -= 1;
                let old_result = cache
                    .get(&maybe_old_query)
                    .map(|x| (maybe_old_query, x.clone()));
                let result = calculate_threshold_from_cache(old_result, query.clone())?;
                cache.insert(query, result.clone());
                result
            }
        };

        // corresponds to right range from apper
        let right_threshold = left_threshold + bin_val.clone();

        // debugging info. Unnecessary
        {
            let right_threshold_float = ToPrimitive::to_f64(&right_threshold.clone());
            let bin_val_float = ToPrimitive::to_f64(&bin_val.clone());
            let normalized_seed_float = ToPrimitive::to_f64(&normalized_seed.clone());
            info!("rightthreshold: {right_threshold_float:?}, bin: {bin_val_float:?}, seed: {normalized_seed_float:?}");
        }

        // from i in 0 to j + 1: B(i; replicas_stake; p)
        if normalized_seed < right_threshold {
            match j {
                0 => return None,
                _ => return Some(NonZeroU64::new(j).unwrap()),
            }
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
    pub fn with_initial_stake(
        known_nodes: Vec<VRFPubKey<SIGSCHEME>>,
        config: &VRFStakeTableConfig,
    ) -> Self {
        assert_eq!(known_nodes.clone().iter().len(), config.distribution.len());
        let key_with_stake = known_nodes.clone()
            .into_iter()
            .map(|x| x.to_bytes())
            .zip(config.distribution.clone())
            .collect();
        // error!("stake table: {:?}", key_with_stake);
        let mut new_known_nodes = Vec::new(); 
        for key in known_nodes.clone() {
            new_known_nodes.push(key.clone().to_bytes());
        }
        VrfImpl {
            stake_table: {
                let st = VRFStakeTable {
                    mapping: key_with_stake,
                    nodes: new_known_nodes,
                    total_stake: NonZeroU64::new(config.distribution.iter().map(|x| x.get()).sum())
                        .unwrap(),
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
            sortition_cache: Arc::default(),
        }
    }
}

/// configuration specifying the stake table
#[derive(Clone, Serialize, Deserialize, core::fmt::Debug)]
pub struct VRFStakeTableConfig {
    /// the committee size parameter
    pub sortition_parameter: NonZeroU64,
    /// the ordered distribution of stake across nodes
    pub distribution: Vec<NonZeroU64>,
}

impl Default for VRFStakeTableConfig {
    fn default() -> Self {
        VRFStakeTableConfig {
            sortition_parameter: NonZeroU64::new(SORTITION_PARAMETER).unwrap(),
            distribution: Vec::new(),
        }
    }
}

impl ElectionConfig for VRFStakeTableConfig {}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroUsize, time::Duration};

    use super::*;
    use ark_bls12_381::Parameters as Param381;
    use ark_std::test_rng;
    use blake3::Hasher;
    use commit::Commitment;
    use hotshot_types::{
        data::{random_commitment, Leaf, ViewNumber},
        traits::state::dummy::DummyState,
    };
    use hotshot_utils::test_util::setup_logging;
    use jf_primitives::{signatures::BLSSignatureScheme, vrf::blsvrf::BLSVRFScheme};

    pub fn gen_vrf_impl(
        num_nodes: usize,
    ) -> (
        VrfImpl<DummyState, BLSSignatureScheme<Param381>, BLSVRFScheme<Param381>, Hasher, Param381>,
        Vec<(
            jf_primitives::signatures::bls::BLSSignKey<Param381>,
            jf_primitives::signatures::bls::BLSVerKey<Param381>,
        )>,
    ) {
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
        let stake_table = VrfImpl::with_initial_stake(
            known_nodes,
            &VRFStakeTableConfig {
                sortition_parameter: std::num::NonZeroU64::new(SORTITION_PARAMETER).unwrap(),
                distribution: stake_distribution,
            },
        );
        (stake_table, keys)
    }

    pub fn check_if_valid<T>(token: &Checked<T>) -> bool {
        match token {
            Checked::Valid(_) => true,
            Checked::Inval(_) | Checked::Unchecked(_) => false,
        }
    }

    #[test]
    pub fn test_sortition() {
        setup_logging();
        let (vrf_impl, keys) = gen_vrf_impl(100);
        let views = 100;

        for view in 0..views {
            let next_state_commitment: Commitment<Leaf<DummyState>> = random_commitment();
            for (node_idx, (sk, pk)) in keys.iter().enumerate() {
                let token_result = vrf_impl
                    .make_vote_token(
                        ViewNumber::new(view),
                        &(sk.clone(), pk.clone()),
                        next_state_commitment,
                    )
                    .unwrap();
                match token_result {
                    Some(token) => {
                        let count = token.count;
                        let start = Instant::now();
                        let result = vrf_impl
                            .validate_vote_token(
                                ViewNumber::new(view),
                                VRFPubKey::from_native(pk.clone()),
                                Checked::Unchecked(token),
                                next_state_commitment,
                            )
                            .unwrap();
                        let duration = Instant::now() - start;
                        println!("Duration for verifying vote token is {:?}", duration);
                        let result_is_valid = check_if_valid(&result);
                        // error!("view {view:?}, node_idx {node_idx:?}, stake {count:?} ");
                        assert!(result_is_valid);
                    }
                    _ => continue,
                }
            }
        }
    }

    #[test]
    pub fn test_factorial() {
        assert_eq!(factorial(0), BigUint::from(1u32));
        assert_eq!(factorial(1), BigUint::from(1u32));
        assert_eq!(factorial(2), BigUint::from(2u32));
        assert_eq!(factorial(3), BigUint::from(6u32));
        assert_eq!(factorial(4), BigUint::from(24u32));
        assert_eq!(factorial(5), BigUint::from(120u32));
    }

    #[test]
    pub fn test_signature_performance() {
        let _seed = [0u8; 32];
        let node_id = 0;
        let vrf_key =
            VRFPubKey::<BLSSignatureScheme<Param381>>::generated_from_seed_indexed(_seed, node_id);
        let priv_key = vrf_key.1;
        let pub_key = VRFPubKey::<BLSSignatureScheme<Param381>>::from_private(&priv_key);

        let data = [3u8; 32];
        let start = Instant::now();
        let signature = VRFPubKey::<BLSSignatureScheme<Param381>>::sign(&priv_key, &data);
        let duration = Instant::now() - start;
        println!("Duration for verifying vote token is {:?}", duration);
    }

    // TODO add failure case

    #[test]
    fn network_config_is_serializable() {
        // validate that `RunResults` can be serialized
        // Note that there is currently an issue with `VRFPubKey` where it can't be serialized with toml
        // so instead we only test with serde_json
        let key =
            <VRFPubKey<BLSSignatureScheme<Param381>> as TestableSignatureKey>::generate_test_key(1);
        let pub_key = VRFPubKey::<BLSSignatureScheme<Param381>>::from_private(&key);
        let mut config = hotshot_centralized_server::NetworkConfig {
            config: hotshot_types::HotShotConfig {
                election_config: Some(super::VRFStakeTableConfig {
                    distribution: vec![NonZeroU64::new(1).unwrap()],
                    sortition_parameter: NonZeroU64::new(1).unwrap(),
                }),
                known_nodes: vec![pub_key],
                execution_type: hotshot_types::ExecutionType::Incremental,
                total_nodes: NonZeroUsize::new(1).unwrap(),
                min_transactions: 1,
                max_transactions: NonZeroUsize::new(1).unwrap(),
                next_view_timeout: 1,
                timeout_ratio: (1, 1),
                round_start_delay: 1,
                start_delay: 1,
                num_bootstrap: 1,
                propose_min_round_time: Duration::from_secs(1),
                propose_max_round_time: Duration::from_secs(1),
            },
            ..Default::default()
        };
        serde_json::to_string(&config).unwrap();
        assert!(toml::to_string(&config).is_err());

        // validate that this is indeed a `pub_key` issue
        config.config.known_nodes.clear();
        serde_json::to_string(&config).unwrap();
        toml::to_string(&config).unwrap();
    }
}
