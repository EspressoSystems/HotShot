use std::sync::Arc;

use hotshot::types::SystemContextHandle;
use hotshot_example_types::node_types::{MemoryImpl, TestTypes};
use hotshot_task_impls::{
    consensus::ConsensusTaskState, events::HotShotEvent, events::HotShotEvent::*,
};
use hotshot_types::traits::node_implementation::NodeType;

pub struct Predicate<INPUT> {
    pub function: Box<dyn Fn(&INPUT) -> bool>,
    pub info: String,
}

impl<INPUT> std::fmt::Debug for Predicate<INPUT> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.info)
    }
}

pub type ConsecutiveEvents<TYPES> = (Arc<HotShotEvent<TYPES>>, Arc<HotShotEvent<TYPES>>);

#[derive(Debug)]
pub enum EventPredicate<TYPES: NodeType> {
    One(Predicate<Arc<HotShotEvent<TYPES>>>),
    Consecutive(Predicate<ConsecutiveEvents<TYPES>>),
}

pub fn exact<TYPES>(event: HotShotEvent<TYPES>) -> EventPredicate<TYPES>
where
    TYPES: NodeType,
{
    let info = format!("{:?}", event);
    let event = Arc::new(event);

    EventPredicate::One(Predicate {
        function: Box::new(move |e| e == &event),
        info,
    })
}

pub fn consecutive<TYPES>(
    events: (HotShotEvent<TYPES>, HotShotEvent<TYPES>),
) -> EventPredicate<TYPES>
where
    TYPES: NodeType,
{
    let info = format!("{:?}", events);
    let (event_0, event_1) = (Arc::new(events.0), Arc::new(events.1));

    EventPredicate::Consecutive(Predicate {
        function: Box::new(move |(e0, e1)| {
            (e0 == &event_0 && e1 == &event_1) || (e0 == &event_1 && e1 == &event_0)
        }),
        info,
    })
}

pub fn multi_exact<TYPES>(
    events: Vec<HotShotEvent<TYPES>>,
) -> Vec<Predicate<Arc<HotShotEvent<TYPES>>>>
where
    TYPES: NodeType,
{
    events
        .into_iter()
        .map(|event| {
            let event = Arc::new(event);
            let info = format!("{:?}", event);
            Predicate {
                function: Box::new(move |e| e == &event),
                info,
            }
        })
        .collect()
}

pub fn leaf_decided<TYPES>() -> EventPredicate<TYPES>
where
    TYPES: NodeType,
{
    let info = "LeafDecided".to_string();
    let function = |e: &Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), LeafDecided(_));

    EventPredicate::One(Predicate {
        function: Box::new(function),
        info,
    })
}

pub fn quorum_vote_send<TYPES>() -> EventPredicate<TYPES>
where
    TYPES: NodeType,
{
    let info = "QuorumVoteSend".to_string();
    let function = |e: &Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), QuorumVoteSend(_));

    EventPredicate::One(Predicate {
        function: Box::new(function),
        info,
    })
}

pub fn view_change<TYPES>() -> EventPredicate<TYPES>
where
    TYPES: NodeType,
{
    let info = "ViewChange".to_string();
    let function = |e: &Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), ViewChange(_));

    EventPredicate::One(Predicate {
        function: Box::new(function),
        info,
    })
}

pub fn upgrade_certificate_formed<TYPES>() -> EventPredicate<TYPES>
where
    TYPES: NodeType,
{
    let info = "UpgradeCertificateFormed".to_string();
    let function = |e: &Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), UpgradeCertificateFormed(_));

    EventPredicate::One(Predicate {
        function: Box::new(function),
        info,
    })
}

pub fn quorum_proposal_send_with_upgrade_certificate<TYPES>() -> EventPredicate<TYPES>
where
    TYPES: NodeType,
{
    let info = "QuorumProposalSend with UpgradeCertificate attached".to_string();
    let function = |e: &Arc<HotShotEvent<TYPES>>| match e.as_ref() {
        QuorumProposalSend(proposal, _) => proposal.data.upgrade_certificate.is_some(),
        _ => false,
    };

    EventPredicate::One(Predicate {
        function: Box::new(function),
        info,
    })
}

pub fn quorum_proposal_validated<TYPES>() -> EventPredicate<TYPES>
where
    TYPES: NodeType,
{
    let info = "QuorumProposalValidated".to_string();
    let function = |e: &Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), QuorumProposalValidated(_));

    EventPredicate::One(Predicate {
        function: Box::new(function),
        info,
    })
}

pub fn quorum_proposal_send<TYPES>() -> EventPredicate<TYPES>
where
    TYPES: NodeType,
{
    let info = "QuorumProposalSend".to_string();
    let function = |e: &Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), QuorumProposalSend(_, _));

    EventPredicate::One(Predicate {
        function: Box::new(function),
        info,
    })
}

pub fn timeout_vote_send<TYPES>() -> EventPredicate<TYPES>
where
    TYPES: NodeType,
{
    let info = "TimeoutVoteSend".to_string();
    let function = |e: &Arc<HotShotEvent<TYPES>>| matches!(e.as_ref(), TimeoutVoteSend(_));

    EventPredicate::One(Predicate {
        function: Box::new(function),
        info,
    })
}

type ConsensusTaskTestState =
    ConsensusTaskState<TestTypes, MemoryImpl, SystemContextHandle<TestTypes, MemoryImpl>>;

pub fn consensus_predicate(
    function: Box<dyn for<'a> Fn(&'a ConsensusTaskTestState) -> bool>,
    info: &str,
) -> Predicate<ConsensusTaskTestState> {
    Predicate {
        function,
        info: info.to_string(),
    }
}

pub fn no_decided_upgrade_cert() -> Predicate<ConsensusTaskTestState> {
    consensus_predicate(
        Box::new(|state| state.decided_upgrade_cert.is_none()),
        "expected decided_upgrade_cert to be None",
    )
}

pub fn decided_upgrade_cert() -> Predicate<ConsensusTaskTestState> {
    consensus_predicate(
        Box::new(|state| state.decided_upgrade_cert.is_some()),
        "expected decided_upgrade_cert to be Some(_)",
    )
}

pub fn is_at_view_number(n: u64) -> Predicate<ConsensusTaskTestState> {
    consensus_predicate(
        Box::new(move |state| *state.cur_view == n),
        format!("expected cur view to be {}", n).as_str(),
    )
}
