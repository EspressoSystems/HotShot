use std::{collections::HashMap, fmt::Debug, marker::PhantomData, sync::Arc};

use crate::{
    events::{HotShotEvent, HotShotTaskCompleted},
    helpers::broadcast_event,
};
use async_broadcast::Sender;
use async_trait::async_trait;
use either::Either::{self, Left, Right};

use hotshot_task::task::{Task, TaskState};
use hotshot_types::{
    simple_certificate::{
        DACertificate, QuorumCertificate, TimeoutCertificate, UpgradeCertificate,
        ViewSyncCommitCertificate2, ViewSyncFinalizeCertificate2, ViewSyncPreCommitCertificate2,
    },
    simple_vote::{
        DAVote, QuorumVote, TimeoutVote, UpgradeVote, ViewSyncCommitVote, ViewSyncFinalizeVote,
        ViewSyncPreCommitVote,
    },
    traits::{election::Membership, node_implementation::NodeType},
    vote::{Certificate, HasViewNumber, Vote, VoteAccumulator},
};
use snafu::Snafu;
use tracing::{debug, error};

#[derive(Snafu, Debug)]
/// Stub of a vote error
pub struct VoteTaskError {}

/// Task state for collecting votes of one type and emiting a certificate
pub struct VoteCollectionTaskState<
    TYPES: NodeType,
    VOTE: Vote<TYPES>,
    CERT: Certificate<TYPES, Voteable = VOTE::Commitment> + Debug,
> {
    /// Public key for this node.
    pub public_key: TYPES::SignatureKey,

    /// Membership for voting
    pub membership: Arc<TYPES::Membership>,

    /// accumulator handles aggregating the votes
    pub accumulator: Option<VoteAccumulator<TYPES, VOTE, CERT>>,

    /// The view which we are collecting votes for
    pub view: TYPES::Time,

    /// Node id
    pub id: u64,
}

/// Describes the functions a vote must implement for it to be aggregatable by the generic vote collection task
pub trait AggregatableVote<
    TYPES: NodeType,
    VOTE: Vote<TYPES>,
    CERT: Certificate<TYPES, Voteable = VOTE::Commitment>,
>
{
    /// return the leader for this votes
    fn get_leader(&self, membership: &TYPES::Membership) -> TYPES::SignatureKey;

    /// return the Hotshot event for the completion of this CERT
    fn make_cert_event(certificate: CERT, key: &TYPES::SignatureKey) -> HotShotEvent<TYPES>;
}

impl<
        TYPES: NodeType,
        VOTE: Vote<TYPES> + AggregatableVote<TYPES, VOTE, CERT>,
        CERT: Certificate<TYPES, Voteable = VOTE::Commitment> + Debug,
    > VoteCollectionTaskState<TYPES, VOTE, CERT>
{
    /// Take one vote and accumultate it. Returns either the cert or the updated state
    /// after the vote is accumulated
    pub async fn accumulate_vote(
        &mut self,
        vote: &VOTE,
        event_stream: &Sender<HotShotEvent<TYPES>>,
    ) -> Option<HotShotTaskCompleted> {
        if vote.get_leader(&self.membership) != self.public_key {
            return None;
        }

        if vote.get_view_number() != self.view {
            error!(
                "Vote view does not match! vote view is {} current view is {}",
                *vote.get_view_number(),
                *self.view
            );
            return None;
        }
        let Some(ref mut accumulator) = self.accumulator else {
            return None;
        };
        match accumulator.accumulate(vote, &self.membership) {
            Either::Left(()) => None,
            Either::Right(cert) => {
                debug!("Certificate Formed! {:?}", cert);

                broadcast_event(VOTE::make_cert_event(cert, &self.public_key), event_stream).await;
                self.accumulator = None;
                Some(HotShotTaskCompleted)
            }
        }
    }
}

impl<
        TYPES: NodeType,
        VOTE: Vote<TYPES>
            + AggregatableVote<TYPES, VOTE, CERT>
            + std::marker::Send
            + std::marker::Sync
            + 'static,
        CERT: Certificate<TYPES, Voteable = VOTE::Commitment>
            + Debug
            + std::marker::Send
            + std::marker::Sync
            + 'static,
    > TaskState for VoteCollectionTaskState<TYPES, VOTE, CERT>
where
    VoteCollectionTaskState<TYPES, VOTE, CERT>: HandleVoteEvent<TYPES, VOTE, CERT>,
{
    type Event = HotShotEvent<TYPES>;

    type Output = HotShotTaskCompleted;

    async fn handle_event(event: Self::Event, task: &mut Task<Self>) -> Option<Self::Output> {
        let sender = task.clone_sender();
        task.state_mut().handle_event(event, &sender).await
    }

    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event, HotShotEvent::Shutdown)
    }
}

/// Trait for types which will handle a vote event.
#[async_trait]
pub trait HandleVoteEvent<TYPES, VOTE, CERT>
where
    TYPES: NodeType,
    VOTE: Vote<TYPES> + AggregatableVote<TYPES, VOTE, CERT>,
    CERT: Certificate<TYPES, Voteable = VOTE::Commitment> + Debug,
{
    /// Handle a vote event
    async fn handle_event(
        &mut self,
        event: HotShotEvent<TYPES>,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> Option<HotShotTaskCompleted>;

    /// Event filter to use for this event
    fn filter(event: &HotShotEvent<TYPES>) -> bool;
}

/// Info needed to create a vote accumulator task
pub struct AccumulatorInfo<TYPES: NodeType> {
    /// This nodes Pub Key
    pub public_key: TYPES::SignatureKey,
    /// Membership we are accumulation votes for
    pub membership: Arc<TYPES::Membership>,
    /// View of the votes we are collecting
    pub view: TYPES::Time,
    /// This nodes id
    pub id: u64,
}

/// Generic function for spawnnig a vote task.  Returns the event stream id of the spawned task if created
/// # Panics
/// Calls unwrap but should never panic.
pub async fn create_vote_accumulator<TYPES, VOTE, CERT>(
    info: &AccumulatorInfo<TYPES>,
    vote: VOTE,
    event: HotShotEvent<TYPES>,
    sender: &Sender<HotShotEvent<TYPES>>,
) -> Option<VoteCollectionTaskState<TYPES, VOTE, CERT>>
where
    TYPES: NodeType,
    VOTE: Vote<TYPES>
        + AggregatableVote<TYPES, VOTE, CERT>
        + std::marker::Send
        + std::marker::Sync
        + 'static,
    CERT: Certificate<TYPES, Voteable = VOTE::Commitment>
        + Debug
        + std::marker::Send
        + std::marker::Sync
        + 'static,
    VoteCollectionTaskState<TYPES, VOTE, CERT>: HandleVoteEvent<TYPES, VOTE, CERT>,
{
    if vote.get_view_number() != info.view {
        error!(
            "Vote view does not match! vote view is {} current view is {}",
            *vote.get_view_number(),
            *info.view
        );
        return None;
    }
    let new_accumulator = VoteAccumulator {
        vote_outcomes: HashMap::new(),
        signers: HashMap::new(),
        phantom: PhantomData,
    };

    let mut state = VoteCollectionTaskState::<TYPES, VOTE, CERT> {
        membership: info.membership.clone(),
        public_key: info.public_key.clone(),
        accumulator: Some(new_accumulator),
        view: info.view,
        id: info.id,
    };

    let result = state.handle_event(event.clone(), sender).await;

    if result == Some(HotShotTaskCompleted) {
        // The protocol has finished
        return None;
    }

    Some(state)
}

/// Alias for Quorum vote accumulator
type QuorumVoteState<TYPES> =
    VoteCollectionTaskState<TYPES, QuorumVote<TYPES>, QuorumCertificate<TYPES>>;
/// Alias for DA vote accumulator
type DAVoteState<TYPES> = VoteCollectionTaskState<TYPES, DAVote<TYPES>, DACertificate<TYPES>>;
/// Alias for Timeout vote accumulator
type TimeoutVoteState<TYPES> =
    VoteCollectionTaskState<TYPES, TimeoutVote<TYPES>, TimeoutCertificate<TYPES>>;
/// Alias for upgrade vote accumulator
type UpgradeVoteState<TYPES> =
    VoteCollectionTaskState<TYPES, UpgradeVote<TYPES>, UpgradeCertificate<TYPES>>;
/// Alias for View Sync Pre Commit vote accumulator
type ViewSyncPreCommitState<TYPES> = VoteCollectionTaskState<
    TYPES,
    ViewSyncPreCommitVote<TYPES>,
    ViewSyncPreCommitCertificate2<TYPES>,
>;
/// Alias for View Sync Commit vote accumulator
type ViewSyncCommitVoteState<TYPES> =
    VoteCollectionTaskState<TYPES, ViewSyncCommitVote<TYPES>, ViewSyncCommitCertificate2<TYPES>>;
/// Alias for View Sync Finalize vote accumulator
type ViewSyncFinalizeVoteState<TYPES> = VoteCollectionTaskState<
    TYPES,
    ViewSyncFinalizeVote<TYPES>,
    ViewSyncFinalizeCertificate2<TYPES>,
>;

impl<TYPES: NodeType> AggregatableVote<TYPES, QuorumVote<TYPES>, QuorumCertificate<TYPES>>
    for QuorumVote<TYPES>
{
    fn get_leader(&self, membership: &TYPES::Membership) -> TYPES::SignatureKey {
        membership.get_leader(self.get_view_number() + 1)
    }
    fn make_cert_event(
        certificate: QuorumCertificate<TYPES>,
        _key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::QCFormed(Left(certificate))
    }
}

impl<TYPES: NodeType> AggregatableVote<TYPES, UpgradeVote<TYPES>, UpgradeCertificate<TYPES>>
    for UpgradeVote<TYPES>
{
    fn get_leader(&self, membership: &TYPES::Membership) -> TYPES::SignatureKey {
        membership.get_leader(self.get_view_number())
    }
    fn make_cert_event(
        certificate: UpgradeCertificate<TYPES>,
        _key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::UpgradeCertificateFormed(certificate)
    }
}

impl<TYPES: NodeType> AggregatableVote<TYPES, DAVote<TYPES>, DACertificate<TYPES>>
    for DAVote<TYPES>
{
    fn get_leader(&self, membership: &TYPES::Membership) -> TYPES::SignatureKey {
        membership.get_leader(self.get_view_number())
    }
    fn make_cert_event(
        certificate: DACertificate<TYPES>,
        key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::DACSend(certificate, key.clone())
    }
}

impl<TYPES: NodeType> AggregatableVote<TYPES, TimeoutVote<TYPES>, TimeoutCertificate<TYPES>>
    for TimeoutVote<TYPES>
{
    fn get_leader(&self, membership: &TYPES::Membership) -> TYPES::SignatureKey {
        membership.get_leader(self.get_view_number() + 1)
    }
    fn make_cert_event(
        certificate: TimeoutCertificate<TYPES>,
        _key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::QCFormed(Right(certificate))
    }
}

impl<TYPES: NodeType>
    AggregatableVote<TYPES, ViewSyncCommitVote<TYPES>, ViewSyncCommitCertificate2<TYPES>>
    for ViewSyncCommitVote<TYPES>
{
    fn get_leader(&self, membership: &TYPES::Membership) -> TYPES::SignatureKey {
        membership.get_leader(self.get_data().round + self.get_data().relay)
    }
    fn make_cert_event(
        certificate: ViewSyncCommitCertificate2<TYPES>,
        key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::ViewSyncCommitCertificate2Send(certificate, key.clone())
    }
}

impl<TYPES: NodeType>
    AggregatableVote<TYPES, ViewSyncPreCommitVote<TYPES>, ViewSyncPreCommitCertificate2<TYPES>>
    for ViewSyncPreCommitVote<TYPES>
{
    fn get_leader(&self, membership: &TYPES::Membership) -> TYPES::SignatureKey {
        membership.get_leader(self.get_data().round + self.get_data().relay)
    }
    fn make_cert_event(
        certificate: ViewSyncPreCommitCertificate2<TYPES>,
        key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::ViewSyncPreCommitCertificate2Send(certificate, key.clone())
    }
}

impl<TYPES: NodeType>
    AggregatableVote<TYPES, ViewSyncFinalizeVote<TYPES>, ViewSyncFinalizeCertificate2<TYPES>>
    for ViewSyncFinalizeVote<TYPES>
{
    fn get_leader(&self, membership: &TYPES::Membership) -> TYPES::SignatureKey {
        membership.get_leader(self.get_data().round + self.get_data().relay)
    }
    fn make_cert_event(
        certificate: ViewSyncFinalizeCertificate2<TYPES>,
        key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::ViewSyncFinalizeCertificate2Send(certificate, key.clone())
    }
}

// Handlers for all vote accumulators
#[async_trait]
impl<TYPES: NodeType> HandleVoteEvent<TYPES, QuorumVote<TYPES>, QuorumCertificate<TYPES>>
    for QuorumVoteState<TYPES>
{
    async fn handle_event(
        &mut self,
        event: HotShotEvent<TYPES>,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> Option<HotShotTaskCompleted> {
        match event {
            HotShotEvent::QuorumVoteRecv(vote) => self.accumulate_vote(&vote, sender).await,
            _ => None,
        }
    }
    fn filter(event: &HotShotEvent<TYPES>) -> bool {
        matches!(event, HotShotEvent::QuorumVoteRecv(_))
    }
}

// Handlers for all vote accumulators
#[async_trait]
impl<TYPES: NodeType> HandleVoteEvent<TYPES, UpgradeVote<TYPES>, UpgradeCertificate<TYPES>>
    for UpgradeVoteState<TYPES>
{
    async fn handle_event(
        &mut self,
        event: HotShotEvent<TYPES>,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> Option<HotShotTaskCompleted> {
        match event {
            HotShotEvent::UpgradeVoteRecv(vote) => self.accumulate_vote(&vote, sender).await,
            _ => None,
        }
    }
    fn filter(event: &HotShotEvent<TYPES>) -> bool {
        matches!(event, HotShotEvent::UpgradeVoteRecv(_))
    }
}

#[async_trait]
impl<TYPES: NodeType> HandleVoteEvent<TYPES, DAVote<TYPES>, DACertificate<TYPES>>
    for DAVoteState<TYPES>
{
    async fn handle_event(
        &mut self,
        event: HotShotEvent<TYPES>,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> Option<HotShotTaskCompleted> {
        match event {
            HotShotEvent::DAVoteRecv(vote) => self.accumulate_vote(&vote, sender).await,
            _ => None,
        }
    }
    fn filter(event: &HotShotEvent<TYPES>) -> bool {
        matches!(event, HotShotEvent::DAVoteRecv(_))
    }
}

#[async_trait]
impl<TYPES: NodeType> HandleVoteEvent<TYPES, TimeoutVote<TYPES>, TimeoutCertificate<TYPES>>
    for TimeoutVoteState<TYPES>
{
    async fn handle_event(
        &mut self,
        event: HotShotEvent<TYPES>,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> Option<HotShotTaskCompleted> {
        match event {
            HotShotEvent::TimeoutVoteRecv(vote) => self.accumulate_vote(&vote, sender).await,
            _ => None,
        }
    }
    fn filter(event: &HotShotEvent<TYPES>) -> bool {
        matches!(event, HotShotEvent::TimeoutVoteRecv(_))
    }
}

#[async_trait]
impl<TYPES: NodeType>
    HandleVoteEvent<TYPES, ViewSyncPreCommitVote<TYPES>, ViewSyncPreCommitCertificate2<TYPES>>
    for ViewSyncPreCommitState<TYPES>
{
    async fn handle_event(
        &mut self,
        event: HotShotEvent<TYPES>,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> Option<HotShotTaskCompleted> {
        match event {
            HotShotEvent::ViewSyncPreCommitVoteRecv(vote) => {
                self.accumulate_vote(&vote, sender).await
            }
            _ => None,
        }
    }
    fn filter(event: &HotShotEvent<TYPES>) -> bool {
        matches!(event, HotShotEvent::ViewSyncPreCommitVoteRecv(_))
    }
}

#[async_trait]
impl<TYPES: NodeType>
    HandleVoteEvent<TYPES, ViewSyncCommitVote<TYPES>, ViewSyncCommitCertificate2<TYPES>>
    for ViewSyncCommitVoteState<TYPES>
{
    async fn handle_event(
        &mut self,
        event: HotShotEvent<TYPES>,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> Option<HotShotTaskCompleted> {
        match event {
            HotShotEvent::ViewSyncCommitVoteRecv(vote) => self.accumulate_vote(&vote, sender).await,
            _ => None,
        }
    }
    fn filter(event: &HotShotEvent<TYPES>) -> bool {
        matches!(event, HotShotEvent::ViewSyncCommitVoteRecv(_))
    }
}

#[async_trait]
impl<TYPES: NodeType>
    HandleVoteEvent<TYPES, ViewSyncFinalizeVote<TYPES>, ViewSyncFinalizeCertificate2<TYPES>>
    for ViewSyncFinalizeVoteState<TYPES>
{
    async fn handle_event(
        &mut self,
        event: HotShotEvent<TYPES>,
        sender: &Sender<HotShotEvent<TYPES>>,
    ) -> Option<HotShotTaskCompleted> {
        match event {
            HotShotEvent::ViewSyncFinalizeVoteRecv(vote) => {
                self.accumulate_vote(&vote, sender).await
            }
            _ => None,
        }
    }
    fn filter(event: &HotShotEvent<TYPES>) -> bool {
        matches!(event, HotShotEvent::ViewSyncFinalizeVoteRecv(_))
    }
}
