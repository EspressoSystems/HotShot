//! Abstract storage type for storing DA proposals and VID shares
//!
//! This modules provides the [`Storage`] trait.
//!

use std::collections::BTreeMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::{
    consensus::{CommitmentMap, View},
    data::{DAProposal, Leaf, VidDisperse},
    event::HotShotAction,
    message::Proposal,
    simple_certificate::QuorumCertificate,
};

use super::node_implementation::NodeType;

/// Abstraction for storing a variety of consensus payload datum.
#[async_trait]
pub trait Storage<TYPES: NodeType>: Send + Sync + Clone {
    async fn append_vid(&self, proposal: &Proposal<TYPES, VidDisperse<TYPES>>) -> Result<()>;
    async fn append_da(&self, proposal: &Proposal<TYPES, DAProposal<TYPES>>) -> Result<()>;
    async fn record_action(&self, view: TYPES::Time, action: HotShotAction) -> Result<()>;
    /// Update the currently undecided state of consensus.  This includes the undecided leaf chain,
    /// the undecided state, and the current high qc.
    async fn update_undecided_state(
        &self,
        high_qc: QuorumCertificate<TYPES>,
        leafs: CommitmentMap<Leaf<TYPES>>,
        state: BTreeMap<TYPES::Time, View<TYPES>>,
    ) -> Result<()>;
}
