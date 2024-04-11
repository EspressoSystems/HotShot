use async_trait::async_trait;
use hotshot_task_impls::events::{HotShotEvent, HotShotEvent::*};
use hotshot_types::{
    data::null_block,
    traits::{block_contents::BlockHeader, node_implementation::NodeType},
};
use std::sync::Arc;

use crate::predicates::{Predicate, PredicateResult};

type BooleanCallback<TYPES> = Arc<dyn Fn(Arc<HotShotEvent<TYPES>>) -> bool + Send + Sync>;

pub struct EventBooleanPredicate<TYPES>
where
    TYPES: NodeType + Send + Sync,
{
    check: BooleanCallback<TYPES>,
    info: String,
}

impl<TYPES: NodeType> std::fmt::Debug for EventBooleanPredicate<TYPES> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.info)
    }
}

#[async_trait]
impl<TYPES> Predicate<Arc<HotShotEvent<TYPES>>> for EventBooleanPredicate<TYPES>
where
    TYPES: NodeType + Send + Sync + 'static,
{
    async fn evaluate(&self, input: &Arc<HotShotEvent<TYPES>>) -> PredicateResult {
        PredicateResult::from((self.check)(input.clone()))
    }

    async fn info(&self) -> String {
        self.info.clone()
    }
}

pub fn exact<TYPES>(event: HotShotEvent<TYPES>) -> Box<EventBooleanPredicate<TYPES>>
where
    TYPES: NodeType,
{
    let info = format!("{:?}", event);
    let event = Arc::new(event);

    let check: BooleanCallback<TYPES> = Arc::new(move |e: Arc<HotShotEvent<TYPES>>| {
        let event_clone = event.clone();
        *e == *event_clone
    });

    Box::new(EventBooleanPredicate { check, info })
}

pub fn leaf_decided<TYPES>() -> Box<EventBooleanPredicate<TYPES>>
where
    TYPES: NodeType,
{
    let info = "LeafDecided".to_string();
    let check: BooleanCallback<TYPES> =
        Arc::new(move |e: Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), LeafDecided(_)));

    Box::new(EventBooleanPredicate { check, info })
}

pub fn quorum_vote_send<TYPES>() -> Box<EventBooleanPredicate<TYPES>>
where
    TYPES: NodeType,
{
    let info = "QuorumVoteSend".to_string();
    let check: BooleanCallback<TYPES> =
        Arc::new(move |e: Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), QuorumVoteSend(_)));

    Box::new(EventBooleanPredicate { check, info })
}

pub fn view_change<TYPES>() -> Box<EventBooleanPredicate<TYPES>>
where
    TYPES: NodeType,
{
    let info = "ViewChange".to_string();
    let check: BooleanCallback<TYPES> =
        Arc::new(move |e: Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), ViewChange(_)));
    Box::new(EventBooleanPredicate { check, info })
}

pub fn upgrade_certificate_formed<TYPES>() -> Box<EventBooleanPredicate<TYPES>>
where
    TYPES: NodeType,
{
    let info = "UpgradeCertificateFormed".to_string();
    let check: BooleanCallback<TYPES> = Arc::new(move |e: Arc<HotShotEvent<TYPES>>| {
        matches!(e.as_ref(), UpgradeCertificateFormed(_))
    });
    Box::new(EventBooleanPredicate { check, info })
}

pub fn quorum_proposal_send_with_upgrade_certificate<TYPES>() -> Box<EventBooleanPredicate<TYPES>>
where
    TYPES: NodeType,
{
    let info = "QuorumProposalSend with UpgradeCertificate attached".to_string();
    let check: BooleanCallback<TYPES> =
        Arc::new(move |e: Arc<HotShotEvent<TYPES>>| match e.as_ref() {
            QuorumProposalSend(proposal, _) => proposal.data.upgrade_certificate.is_some(),
            _ => false,
        });
    Box::new(EventBooleanPredicate { info, check })
}

pub fn quorum_proposal_validated<TYPES>() -> Box<EventBooleanPredicate<TYPES>>
where
    TYPES: NodeType,
{
    let info = "QuorumProposalValidated".to_string();
    let check: BooleanCallback<TYPES> = Arc::new(move |e: Arc<HotShotEvent<TYPES>>| {
        matches!(*e.clone(), QuorumProposalValidated(..))
    });
    Box::new(EventBooleanPredicate { check, info })
}

pub fn quorum_proposal_send<TYPES>() -> Box<EventBooleanPredicate<TYPES>>
where
    TYPES: NodeType,
{
    let info = "QuorumProposalSend".to_string();
    let check: BooleanCallback<TYPES> =
        Arc::new(move |e: Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), QuorumProposalSend(..)));
    Box::new(EventBooleanPredicate { check, info })
}

pub fn quorum_proposal_send_with_null_block<TYPES>(
    num_storage_nodes: usize,
) -> Box<EventBooleanPredicate<TYPES>>
where
    TYPES: NodeType,
{
    let info = "QuorumProposalSend with null block payload".to_string();
    let check: BooleanCallback<TYPES> =
        Arc::new(move |e: Arc<HotShotEvent<TYPES>>| match e.as_ref() {
            QuorumProposalSend(proposal, _) => {
                Some(proposal.data.block_header.payload_commitment())
                    == null_block::commitment(num_storage_nodes)
            }
            _ => false,
        });
    Box::new(EventBooleanPredicate { check, info })
}

pub fn timeout_vote_send<TYPES>() -> Box<EventBooleanPredicate<TYPES>>
where
    TYPES: NodeType,
{
    let info = "TimeoutVoteSend".to_string();
    let check: BooleanCallback<TYPES> =
        Arc::new(move |e: Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), TimeoutVoteSend(..)));
    Box::new(EventBooleanPredicate { check, info })
}
