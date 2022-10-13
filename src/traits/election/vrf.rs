use std::{marker::PhantomData, collections::BTreeMap, num::NonZeroU64};

use hotshot_types::traits::{signature_key::{SignatureKey, EncodedPublicKey}, state::ConsensusTime, election::{Election, VoteToken, ElectionError, Checked}, State};
use hotshot_utils::{hack::nll_todo, bincode::bincode_opts};
use jf_primitives::{signatures::{
    bls::{BLSSignKey, BLSSignature, BLSSignatureScheme, BLSVerKey},
    SignatureScheme,
}, vrf::Vrf};
use serde::{Serialize, Deserialize, de::DeserializeOwned};

// TODO abstraction this function's impl into a trait
// TODO do we necessariy want the units of stake to be a u64? or generics
#[derive(Serialize, Deserialize, Clone)]
pub struct VRFStakeTable<VRF, VRFHASHER, VRFPARAMS>
{
    mapping: BTreeMap<EncodedPublicKey, NonZeroU64>,
    total_stake: u64,
    _pd_0: PhantomData<VRF>,
    _pd_1: PhantomData<VRFHASHER>,
    _pd_2: PhantomData<VRFPARAMS>,
}

impl<VRF, VRFHASHER, VRFPARAMS> VRFStakeTable<VRF, VRFHASHER, VRFPARAMS>
    where VRF: Vrf<VRFHASHER, VRFPARAMS>,
          <VRF as Vrf<VRFHASHER, VRFPARAMS>>::PublicKey : SignatureKey {
    pub fn get_all_stake(&self) -> u64 {
        self.total_stake
    }
    pub fn get_stake(&self, pk: &VRF::PublicKey) -> Option<u64> {
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
#[derive(Clone)]
pub struct VrfImpl<
    STATE,
    VRF,
    VRFHASHER,
    VRFPARAMS,
>
where STATE: State,
    VRF::PublicKey : Clone + Sync + Send + Serialize + DeserializeOwned + SignatureKey<PrivateKey = VRF::SecretKey>,
    VRF::Proof : Clone + Sync + Send + Serialize + DeserializeOwned,
    VRFHASHER: Clone + Sync + Send + Serialize + DeserializeOwned,
    VRFPARAMS: Clone + Sync + Send + Serialize + DeserializeOwned,
    VRF: Vrf<VRFHASHER, VRFPARAMS> + Clone + Sync + Send,
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
}

pub fn get_total_stake() {
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VRFVoteToken<
    VRF: Vrf<VRFHASHER, VRFPARAMS>,
    VRFHASHER,
    VRFPARAMS,
> {
    /// The public key assocaited with this token
    pub pub_key: VRF::PublicKey,
    /// The list of signatures
    pub proof: VRF::Proof,
    /// The number of signatures that are valid
    /// TODO (ct) this should be the sorition outbput
    pub count: u64,
}

impl<VRF, VRFHASHER, VRFPARAMS> VoteToken for VRFVoteToken<VRF, VRFHASHER, VRFPARAMS>
    where VRF: Vrf<VRFHASHER, VRFPARAMS> {
    fn vote_count(&self) -> u64 {
        self.count
    }
}

// KEY is VRFPubKey
impl<
    VRFHASHER,
    VRFPARAMS,
    VRF,
    TIME,
    STATE
> Election<VRF::PublicKey, TIME> for VrfImpl<STATE, VRF, VRFHASHER, VRFPARAMS>
where
    VRF: Vrf<VRFHASHER, VRFPARAMS, Input = [u8; 32]> + Clone + Sync + Send,
    VRF::PublicKey : SignatureKey<PrivateKey = VRF::SecretKey> + Ord,
    VRF::Proof : Clone + Sync + Send + Serialize + DeserializeOwned,
    VRF::PublicParameter: Sync + Send,
    VRFHASHER: Clone + Sync + Send + Serialize + DeserializeOwned,
    VRFPARAMS: Clone + Sync + Send + Serialize + DeserializeOwned,
    TIME: ConsensusTime,
    STATE: State,
{
    // pubkey -> unit of stake
    type StakeTable = VRFStakeTable<VRF, VRFHASHER, VRFPARAMS>;

    type StateType = STATE;

    // TODO generics in terms of vrf trait output(s)
    // represents a vote on a proposal
    type VoteTokenType = VRFVoteToken<VRF, VRFHASHER, VRFPARAMS>;

    // FIXED STAKE
    // just return the state
    fn get_stake_table(&self, _view_number: hotshot_types::data::ViewNumber, _state: &Self::StateType) -> Self::StakeTable {
        self.stake_table.clone()
    }

    fn get_leader(&self, view_number: hotshot_types::data::ViewNumber) -> VRF::PublicKey {
        // TODO fst2 (ct) this is round robin, we should make this dependent on
        // the VRF + some source of randomness

        // TODO for now do by stake table of how much stake each
        // participant has
        let mapping = &self.stake_table.mapping;
        let index = ((*view_number) as usize) % mapping.len();
        let encoded = mapping.keys().nth(index).unwrap();
        SignatureKey::from_bytes(encoded).unwrap()
    }

    fn check_threshold(
            &self,
            _signatures: &BTreeMap<EncodedPublicKey, (hotshot_types::traits::signature_key::EncodedSignature, Vec<u8>)>,
            _threshold: std::num::NonZeroUsize,
        ) -> bool {
        nll_todo()
    }

    // what this is doing:
    // -
    fn make_vote_token(
        // TODO see if we can make this take &mut self
        // because we're using a mutable prng
        &self,
        view_number: hotshot_types::data::ViewNumber,
        private_key: &VRF::SecretKey,
        // TODO (ct) this should be replaced with something else...
        next_state: commit::Commitment<hotshot_types::data::Leaf<Self::StateType>>,
    ) -> Result<Option<Self::VoteTokenType>, hotshot_types::traits::election::ElectionError> {
        let pub_key = <VRF::PublicKey as SignatureKey>::from_private(&private_key);
        // TODO (ct) what should state be?
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
                    private_key,
                    &<[u8; 32]>::from(next_state),
                    &mut *self.prng.lock().unwrap()
                ).unwrap();

                Ok(Some(VRFVoteToken {
                    pub_key,
                    proof,
                    count,
                }))

            },
            None => Ok(None),
        }
    }

    fn validate_vote_token(
        &self,
        view_number: hotshot_types::data::ViewNumber,
        pub_key: VRF::PublicKey,
        token: Checked<Self::VoteTokenType>,
        next_state: commit::Commitment<hotshot_types::data::Leaf<Self::StateType>>,
    ) -> Result<Checked<Self::VoteTokenType>, hotshot_types::traits::election::ElectionError> {
        match token {
            Checked::Unchecked(token) => {
                let pubkey = nll_todo();
                let stake : Option<u64> = nll_todo();
                if let Some(stake) = stake {
                    if token.count != stake {
                        return Err(ElectionError::StubError);
                    }
                    if let Ok(r) = VRF::verify(&self.proof_parameters, &token.proof, &pubkey, &<[u8; 32]>::from(next_state)) {
                        Ok(Checked::Valid(token))
                    } else {
                        Ok(Checked::Inval(token))
                    }
                } else {
                    // TODO better error
                    Err(ElectionError::StubError)
                }
            },
            already_checked => Ok(already_checked)
        }
    }

}
fn calculate_stake(stake: u64) -> Option<u64> {
    Some(stake)
}

