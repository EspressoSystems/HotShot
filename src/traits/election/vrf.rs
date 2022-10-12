use std::marker::PhantomData;

use hotshot_types::traits::{signature_key::SignatureKey, state::ConsensusTime, election::Election, State};
use jf_primitives::{signatures::{
    bls::{BLSSignKey, BLSSignature, BLSSignatureScheme, BLSVerKey},
    SignatureScheme,
}, vrf::Vrf};

/// TODO this may not be correct for KEY
pub struct VrfImpl<HASH: Sized, KEY: SignatureKey, VRF: Vrf<HASH, KEY>> {
    vrf: VRF,
    _pd_0: PhantomData<HASH>,
    _pd_1: PhantomData<KEY>,
}

pub struct VRFVoteToken<
    KEY: SignatureKey,
    HASH: Sized + Send + Sync + 'static ,
    VRF: Vrf<HASH, KEY> + Send + Sync + 'static
> {
    /// The public key assocaited with this token
    pub pub_key: KEY,
    /// The list of signatures
    pub proof: VRF::Proof,
    /// The number of signatures that are valid
    /// TODO (ct) this should be sorition outbput
    pub count: u64,
}

// KEY is BLSPubKey
impl<STATE: State, HASH: Sized + Send + Sync + 'static , VRF: Vrf<HASH, KEY> + Send + Sync + 'static, KEY: SignatureKey, TIME: ConsensusTime> Election<KEY, TIME> for VrfImpl<HASH, KEY, VRF> {
    type StakeTable: BTreeMap<KEY, NonZeroU64>,

    type StateType = STATE;

    // TODO generics in terms of vrf trait output(s)
    // represents a vote on a proposal
    type VoteTokenType = VRFVoteToken<_>;

    fn get_stake_table(&self, view_number: hotshot_types::data::ViewNumber, state: &Self::StateType) -> Self::StakeTable {
        nll_todo()
    }

    fn get_leader(&self, view_number: hotshot_types::data::ViewNumber) -> KEY {
        nll_todo()
    }

    fn make_vote_token(
        &self,
        view_number: hotshot_types::data::ViewNumber,
        private_key: &<KEY as SignatureKey>::PrivateKey,
        // TODO (ct) this should be replaced with something else...
        next_state: commit::Commitment<hotshot_types::data::Leaf<Self::StateType>>,
    ) -> Result<Option<Self::VoteTokenType>, hotshot_types::traits::election::ElectionError> {
        nll_todo()
    }

    fn validate_vote_token(
        &self,
        view_number: hotshot_types::data::ViewNumber,
        pub_key: KEY,
        token: Self::VoteTokenType,
    ) -> Result<hotshot_types::traits::election::Checked<Self::VoteTokenType>, hotshot_types::traits::election::ElectionError> {
        nll_todo()
    }
}
