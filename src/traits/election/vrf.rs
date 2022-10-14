use ark_ec::bls12::Bls12Parameters;
use bincode::Options;
use hotshot_types::traits::{
    election::{Checked, Election, ElectionError, VoteToken},
    signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
    state::ConsensusTime,
    State,
};
use hotshot_utils::{bincode::bincode_opts, hack::nll_todo};
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
    // #[serde(ignore)]
    prng: std::sync::Arc<std::sync::Mutex<rand_chacha::ChaChaRng>>,
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
        let my_stake = match self.stake_table.get_stake(&pub_key) {
            Some(val) => val,
            None => return Ok(None),
        };
        // calculate hash / 2^ thing
        // iterate through buckets and pick the correct one
        let my_view_selected_stake = calculate_stake(my_stake);
        match my_view_selected_stake {
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
                let pubkey = nll_todo();
                let stake: Option<u64> = nll_todo();
                if let Some(stake) = stake {
                    if token.count != stake {
                        return Err(ElectionError::StubError);
                    }
                    if let Ok(r) = VRF::verify(
                        &self.proof_parameters,
                        &token.proof,
                        &pubkey,
                        &<[u8; 32]>::from(next_state),
                    ) {
                        Ok(Checked::Valid(token))
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

fn calculate_stake(stake: u64) -> Option<u64> {
    Some(stake)
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
    pub fn with_initial_stake(known_nodes: Vec<VRFPubKey<SIGSCHEME>>) -> Self {
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
