use std::{marker::PhantomData, collections::BTreeMap, num::NonZeroU64};

use hotshot_types::traits::{signature_key::SignatureKey, state::ConsensusTime, election::{Election, VoteToken}, State};
use hotshot_utils::hack::nll_todo;
use jf_primitives::{signatures::{
    bls::{BLSSignKey, BLSSignature, BLSSignatureScheme, BLSVerKey},
    SignatureScheme,
}, vrf::Vrf};
use serde::{Serialize, Deserialize, de::DeserializeOwned};

/// TODO this may not be correct for KEY
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VrfImpl<
    STATE,
    VRF,
    VRFHASHER,
    VRFPARAMS,
>
where STATE: State,
      VRF: Vrf<VRFHASHER, VRFPARAMS>,


{
    // TODO (fst2) accessor to stake table
    // stake_table:
    _pd_0: PhantomData<VRFHASHER>,
    _pd_1: PhantomData<VRFPARAMS>,
    _pd_2: PhantomData<STATE>,
    _pd_3: PhantomData<VRF>,
}

// pub type VRFStakeTable =

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VRFVoteToken<
    VRFPARAMS,
    VRFHASHER,
    VRF: Vrf<VRFHASHER, VRFPARAMS>,
> {
    /// The public key assocaited with this token
    pub pub_key: VRF::PublicKey,
    /// The list of signatures
    pub proof: VRF::Proof,
    /// The number of signatures that are valid
    /// TODO (ct) this should be the sorition outbput
    pub count: u64,
}

// TODO parametrize only on the public key
#[derive(Serialize, Deserialize, Clone)]
pub struct VRFPubKey<VRF, VRFHASHER, VRFPARAMS>
where
  VRF: Vrf<VRFHASHER, VRFPARAMS>,
{
    inner: VRF::PublicKey
}

impl<VRF, VRFHASHER, VRFPARAMS> std::hash::Hash for VRFPubKey<VRF, VRFHASHER, VRFPARAMS>
where
  VRF: Vrf<VRFHASHER, VRFPARAMS>,
{
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // self.to_bytes().hash(state);
        nll_todo()
    }
}

impl <VRF, VRFHASHER, VRFPARAMS>std::fmt::Debug for VRFPubKey<VRF, VRFHASHER, VRFPARAMS>
where
  VRF: Vrf<VRFHASHER, VRFPARAMS>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        nll_todo()
        // f.debug_struct("VRFPubKey")
        //     .field("pub_key", &"A Public Key!")
        //     .finish()
    }
}

impl<VRF, VRFHASHER, VRFPARAMS> PartialEq for VRFPubKey<VRF, VRFHASHER, VRFPARAMS>
where
  VRF: Vrf<VRFHASHER, VRFPARAMS>,
{
    fn eq(&self, other: &Self) -> bool {
        nll_todo()
        // self.to_bytes() == other.to_bytes()
    }
}

impl<VRF, VRFHASHER, VRFPARAMS> Eq for VRFPubKey<VRF, VRFHASHER, VRFPARAMS>
where
  VRF: Vrf<VRFHASHER, VRFPARAMS>,
{}

impl<VRF, VRFHASHER, VRFPARAMS> Ord for VRFPubKey<VRF, VRFHASHER, VRFPARAMS>
where
  VRF: Vrf<VRFHASHER, VRFPARAMS>,
{
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // self.to_bytes().cmp(&other.to_bytes())
        nll_todo()
    }
}

impl<VRF, VRFHASHER, VRFPARAMS> PartialOrd for VRFPubKey<VRF, VRFHASHER, VRFPARAMS>
where
  VRF: Vrf<VRFHASHER, VRFPARAMS>,
{
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        nll_todo()
        // self.to_bytes().partial_cmp(&other.to_bytes())
    }
}

impl<VRF, VRFHASHER, VRFPARAMS> SignatureKey for VRFPubKey<VRF, VRFHASHER, VRFPARAMS>
where
    VRF::PublicKey : Clone + Sync + Send + Serialize + DeserializeOwned,
    VRFHASHER: Clone + Sync + Send + Serialize + DeserializeOwned,
    VRFPARAMS: Clone + Sync + Send + Serialize + DeserializeOwned,
    VRF: Vrf<VRFHASHER, VRFPARAMS> + Clone + Sync + Send,
{
    // TODO
    type PrivateKey = VRFPrivKey<VRF, VRFHASHER, VRFPARAMS>;

    fn validate(&self, signature: &hotshot_types::traits::signature_key::EncodedSignature, data: &[u8]) -> bool {
        nll_todo()
    }

    fn sign(private_key: &Self::PrivateKey, data: &[u8]) -> hotshot_types::traits::signature_key::EncodedSignature {
        nll_todo()
    }

    fn from_private(private_key: &Self::PrivateKey) -> Self {
        nll_todo()
    }

    fn to_bytes(&self) -> hotshot_types::traits::signature_key::EncodedPublicKey {
        nll_todo()
    }

    fn from_bytes(bytes: &hotshot_types::traits::signature_key::EncodedPublicKey) -> Option<Self> {
        nll_todo()
    }
}


#[derive(Serialize, Deserialize)]
pub struct VRFPrivKey<VRF, VRFHASHER, VRFPARAMS>
where
  VRF: Vrf<VRFHASHER, VRFPARAMS> + Clone + Sync + Send,
{
    pub_key: VRF::PublicKey,
    priv_key: VRF::SecretKey
}

unsafe impl<VRF, VRFHASHER, VRFPARAMS> Send for VRFPrivKey<VRF, VRFHASHER, VRFPARAMS>
where
  VRF: Vrf<VRFHASHER, VRFPARAMS> + Clone + Sync + Send,
{ }

unsafe impl<VRF, VRFHASHER, VRFPARAMS> Sync for VRFPrivKey<VRF, VRFHASHER, VRFPARAMS>
where
  VRF: Vrf<VRFHASHER, VRFPARAMS> + Clone + Sync + Send,
{ }

// impl VoteToken for VRFVoteToken<>

impl<VRFPARAMS, VRFHASHER, VRF> VoteToken for VRFVoteToken<VRFPARAMS, VRFHASHER, VRF>
where
    VRF::PublicKey : Clone + Sync + Send + Serialize + DeserializeOwned,
    VRFHASHER: Clone + Sync + Send + Serialize + DeserializeOwned,
    VRFPARAMS: Clone + Sync + Send + Serialize + DeserializeOwned,
    VRF: Vrf<VRFHASHER, VRFPARAMS> + Clone + Sync + Send,
{
    fn vote_count(&self) -> u64 {
        nll_todo()
    }
}


// KEY is VRFPubKey
impl<
    VRFHASHER,
    VRFPARAMS,
    VRF,
    TIME,
    STATE
> Election<VRFPubKey<VRF, VRFHASHER, VRFPARAMS>, TIME> for VrfImpl<STATE, VRF, VRFHASHER, VRFPARAMS>
where
    VRF::PublicKey : Clone + Sync + Send + Serialize + DeserializeOwned,
    VRF::Proof : Clone + Sync + Send + Serialize + DeserializeOwned,
    VRFHASHER: Clone + Sync + Send + Serialize + DeserializeOwned,
    VRFPARAMS: Clone + Sync + Send + Serialize + DeserializeOwned,
    VRF: Vrf<VRFHASHER, VRFPARAMS> + Clone + Sync + Send,
    TIME: ConsensusTime,
    STATE: State,
{
    // pubkey -> unit of stake
    type StakeTable = BTreeMap<VRFPubKey<VRF, VRFHASHER, VRFPARAMS>, NonZeroU64>;

    type StateType = STATE;

    // TODO generics in terms of vrf trait output(s)
    // represents a vote on a proposal
    type VoteTokenType = VRFVoteToken<VRFPARAMS, VRFHASHER, VRF>;

    fn get_stake_table(&self, view_number: hotshot_types::data::ViewNumber, state: &Self::StateType) -> Self::StakeTable {
        nll_todo()
    }

    fn get_leader(&self, view_number: hotshot_types::data::ViewNumber) -> VRFPubKey<VRF, VRFHASHER, VRFPARAMS> {
        nll_todo()
    }

    fn make_vote_token(
        &self,
        view_number: hotshot_types::data::ViewNumber,
        private_key: &VRFPrivKey<VRF, VRFHASHER, VRFPARAMS>,
        // TODO (ct) this should be replaced with something else...
        next_state: commit::Commitment<hotshot_types::data::Leaf<Self::StateType>>,
    ) -> Result<Option<Self::VoteTokenType>, hotshot_types::traits::election::ElectionError> {
        nll_todo()
    }

    fn validate_vote_token(
        &self,
        view_number: hotshot_types::data::ViewNumber,
        pub_key: VRFPubKey<VRF, VRFHASHER, VRFPARAMS>,
        token: Self::VoteTokenType,
    ) -> Result<hotshot_types::traits::election::Checked<Self::VoteTokenType>, hotshot_types::traits::election::ElectionError> {
        nll_todo()
    }

}
