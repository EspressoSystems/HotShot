#![allow(dead_code)]
#![allow(clippy::missing_docs_in_private_items)]
#![allow(missing_docs)]

use std::{
    fmt::{self, Debug, Display, Formatter},
    hash::Hash,
    marker::PhantomData,
};

use commit::{Commitment, CommitmentBoundsArkless, Committable};
use ethereum_types::U256;

use crate::{
    simple_vote::{Voteable, YesData},
    traits::{
        election::Membership, node_implementation::NodeType, signature_key::SignatureKey,
        state::ConsensusTime,
    },
    vote2::{Certificate2, HasViewNumber},
};

use serde::{Deserialize, Serialize};

/// A certificate which can be created by aggregating many simple votes on the commitment.
#[derive(Serialize, Deserialize, Eq, Hash, PartialEq, Debug, Clone)]
pub struct SimpleCertificate<TYPES: NodeType, VOTEABLE: Voteable> {
    /// commitment to previous leaf which all the votes in this certificate are voting on
    pub leaf_commitment: VOTEABLE,
    /// commitment of all the votes this cert should be signed over
    pub vote_commitment: Commitment<VOTEABLE>,
    /// Which view this QC relates to
    pub view_number: TYPES::Time,
    /// assembled signature for certificate aggregation
    pub signatures: Option<<TYPES::SignatureKey as SignatureKey>::QCType>,
    /// If this QC is for the genesis block
    pub is_genesis: bool,
    /// phantom data for `MEMBERSHIP` and `TYPES`
    pub _pd: PhantomData<TYPES>,
}

impl<TYPES: NodeType, VOTEABLE: Voteable + 'static> Certificate2<TYPES>
    for SimpleCertificate<TYPES, VOTEABLE>
{
    type Voteable = VOTEABLE;
    // type Membership = MEMBERSHIP;

    fn create_signed_certificate(
        vote_commitment: Commitment<VOTEABLE>,
        data: Self::Voteable,
        sig: <TYPES::SignatureKey as SignatureKey>::QCType,
        view: TYPES::Time,
    ) -> Self {
        SimpleCertificate {
            leaf_commitment: data,
            vote_commitment,
            view_number: view,
            signatures: Some(sig),
            is_genesis: false,
            _pd: PhantomData,
        }
    }
    fn is_valid_cert<MEMBERSHIP: Membership<TYPES>>(&self, membership: &MEMBERSHIP) -> bool {
        if self.is_genesis && self.view_number == TYPES::Time::genesis() {
            return true;
        }
        let real_qc_pp = <TYPES::SignatureKey as SignatureKey>::get_public_parameter(
            membership.get_committee_qc_stake_table(),
            U256::from(membership.success_threshold().get()),
        );
        <TYPES::SignatureKey as SignatureKey>::check(
            &real_qc_pp,
            self.vote_commitment.as_ref(),
            self.signatures.as_ref().unwrap(),
        )
    }
    fn threshold<MEMBERSHIP: Membership<TYPES>>(membership: &MEMBERSHIP) -> u64 {
        membership.success_threshold().into()
    }
    fn get_data(&self) -> &Self::Voteable {
        &self.leaf_commitment
    }
    fn get_data_commitment(&self) -> Commitment<Self::Voteable> {
        self.vote_commitment
    }
}

impl<TYPES: NodeType, VOTEABLE: Voteable + 'static> HasViewNumber<TYPES>
    for SimpleCertificate<TYPES, VOTEABLE>
{
    fn get_view_number(&self) -> TYPES::Time {
        self.view_number
    }
}
impl<TYPES: NodeType, VOTEABLE: Voteable + 'static> Display
    for QuorumCertificate2<TYPES, VOTEABLE>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "view: {:?}, is_genesis: {:?}",
            self.view_number, self.is_genesis
        )
    }
}

impl<
        TYPES: NodeType,
        LEAF: Committable + Committable + Clone + Serialize + Debug + PartialEq + Hash + Eq + 'static,
    > SimpleCertificate<TYPES, YesData<LEAF>>
{
    pub fn genesis() -> Self {
        let data = YesData {
            leaf_commit: Commitment::<LEAF>::default_commitment_no_preimage(),
        };
        let commit = data.commit();
        Self {
            leaf_commitment: data,
            vote_commitment: commit,
            view_number: <TYPES::Time as ConsensusTime>::genesis(),
            signatures: None,
            is_genesis: true,
            _pd: PhantomData,
        }
    }
}

// Type aliases for simple use of all the main votes.  We should never see `SimpleVote` outside this file
pub type QuorumCertificate2<TYPES, LEAF> = SimpleCertificate<TYPES, YesData<LEAF>>;
