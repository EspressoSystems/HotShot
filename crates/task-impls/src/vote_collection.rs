// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{
    collections::{btree_map::Entry, BTreeMap, HashMap},
    fmt::Debug,
    marker::PhantomData,
    sync::Arc,
};

use async_broadcast::Sender;
use async_trait::async_trait;
use either::Either::{self, Left, Right};
use hotshot_types::{
    message::UpgradeLock,
    simple_certificate::{
        DaCertificate, QuorumCertificate, TimeoutCertificate, UpgradeCertificate,
        ViewSyncCommitCertificate2, ViewSyncFinalizeCertificate2, ViewSyncPreCommitCertificate2,
    },
    simple_vote::{
        DaVote, QuorumVote, TimeoutVote, UpgradeVote, ViewSyncCommitVote, ViewSyncFinalizeVote,
        ViewSyncPreCommitVote,
    },
    traits::{
        election::Membership,
        node_implementation::{NodeType, Versions},
    },
    vote::{Certificate, HasViewNumber, Vote, VoteAccumulator},
};
use utils::anytrace::*;

use crate::{events::HotShotEvent, helpers::broadcast_event};

/// Alias for a map of Vote Collectors
pub type VoteCollectorsMap<TYPES, VOTE, CERT, V> =
    BTreeMap<<TYPES as NodeType>::View, VoteCollectionTaskState<TYPES, VOTE, CERT, V>>;

/// Task state for collecting votes of one type and emitting a certificate
pub struct VoteCollectionTaskState<
    TYPES: NodeType,
    VOTE: Vote<TYPES>,
    CERT: Certificate<TYPES, Voteable = VOTE::Commitment> + Debug,
    V: Versions,
> {
    /// Public key for this node.
    pub public_key: TYPES::SignatureKey,

    /// Membership for voting
    pub membership: Arc<TYPES::Membership>,

    /// accumulator handles aggregating the votes
    pub accumulator: Option<VoteAccumulator<TYPES, VOTE, CERT, V>>,

    /// The view which we are collecting votes for
    pub view: TYPES::View,

    /// The epoch which we are collecting votes for
    pub epoch: TYPES::Epoch,

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
    ///
    /// # Errors
    /// if the leader cannot be calculated
    fn leader(
        &self,
        membership: &TYPES::Membership,
        epoch: TYPES::Epoch,
    ) -> Result<TYPES::SignatureKey>;

    /// return the Hotshot event for the completion of this CERT
    fn make_cert_event(certificate: CERT, key: &TYPES::SignatureKey) -> HotShotEvent<TYPES>;
}

impl<
        TYPES: NodeType,
        VOTE: Vote<TYPES> + AggregatableVote<TYPES, VOTE, CERT>,
        CERT: Certificate<TYPES, Voteable = VOTE::Commitment> + Clone + Debug,
        V: Versions,
    > VoteCollectionTaskState<TYPES, VOTE, CERT, V>
{
    /// Take one vote and accumulate it. Returns either the cert or the updated state
    /// after the vote is accumulated
    ///
    /// # Errors
    /// If are unable to accumulate the vote
    #[allow(clippy::question_mark)]
    pub async fn accumulate_vote(
        &mut self,
        vote: &VOTE,
        event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<Option<CERT>> {
        ensure!(
            vote.leader(&self.membership, self.epoch)? == self.public_key,
            info!("Received vote for a view in which we were not the leader.")
        );
        ensure!(
            vote.view_number() == self.view,
            error!(
                "Vote view does not match! vote view is {} current view is {}. This vote should not have been passed to this accumulator.",
                *vote.view_number(),
                *self.view
            )
        );

        let accumulator = self.accumulator.as_mut().context(warn!(
            "No accumulator to handle vote with. This shouldn't happen."
        ))?;

        match accumulator
            .accumulate(vote, &self.membership, self.epoch)
            .await
        {
            Either::Left(()) => Ok(None),
            Either::Right(cert) => {
                tracing::debug!("Certificate Formed! {:?}", cert);

                broadcast_event(
                    Arc::new(VOTE::make_cert_event(cert.clone(), &self.public_key)),
                    event_stream,
                )
                .await;
                self.accumulator = None;

                Ok(Some(cert))
            }
        }
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
    ///
    /// # Errors
    /// Returns an error if we fail to handle the vote
    async fn handle_vote_event(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<Option<CERT>>;

    /// Event filter to use for this event
    fn filter(event: Arc<HotShotEvent<TYPES>>) -> bool;
}

/// Info needed to create a vote accumulator task
pub struct AccumulatorInfo<TYPES: NodeType> {
    /// This nodes Pub Key
    pub public_key: TYPES::SignatureKey,
    /// Membership we are accumulation votes for
    pub membership: Arc<TYPES::Membership>,
    /// View of the votes we are collecting
    pub view: TYPES::View,
    /// Epoch of the votes we are collecting
    pub epoch: TYPES::Epoch,
    /// This nodes id
    pub id: u64,
}

/// Generic function for spawning a vote task.  Returns the event stream id of the spawned task if created
///
/// # Errors
/// If we faile to create the accumulator
///
/// # Panics
/// Calls unwrap but should never panic.
pub async fn create_vote_accumulator<TYPES, VOTE, CERT, V>(
    info: &AccumulatorInfo<TYPES>,
    event: Arc<HotShotEvent<TYPES>>,
    sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    upgrade_lock: UpgradeLock<TYPES, V>,
) -> Result<VoteCollectionTaskState<TYPES, VOTE, CERT, V>>
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
    V: Versions,
    VoteCollectionTaskState<TYPES, VOTE, CERT, V>: HandleVoteEvent<TYPES, VOTE, CERT>,
{
    let new_accumulator = VoteAccumulator {
        vote_outcomes: HashMap::new(),
        signers: HashMap::new(),
        phantom: PhantomData,
        upgrade_lock,
    };

    let mut state = VoteCollectionTaskState::<TYPES, VOTE, CERT, V> {
        membership: Arc::clone(&info.membership),
        public_key: info.public_key.clone(),
        accumulator: Some(new_accumulator),
        view: info.view,
        epoch: info.epoch,
        id: info.id,
    };

    state.handle_vote_event(Arc::clone(&event), sender).await?;

    Ok(state)
}

/// A helper function that handles a vote regardless whether it's the first vote in the view or not.
///
/// # Errors
/// If we fail to handle the vote
#[allow(clippy::too_many_arguments)]
pub async fn handle_vote<
    TYPES: NodeType,
    VOTE: Vote<TYPES> + AggregatableVote<TYPES, VOTE, CERT> + Send + Sync + 'static,
    CERT: Certificate<TYPES, Voteable = VOTE::Commitment> + Debug + Send + Sync + 'static,
    V: Versions,
>(
    collectors: &mut VoteCollectorsMap<TYPES, VOTE, CERT, V>,
    vote: &VOTE,
    public_key: TYPES::SignatureKey,
    membership: &Arc<TYPES::Membership>,
    epoch: TYPES::Epoch,
    id: u64,
    event: &Arc<HotShotEvent<TYPES>>,
    event_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
    upgrade_lock: &UpgradeLock<TYPES, V>,
) -> Result<()>
where
    VoteCollectionTaskState<TYPES, VOTE, CERT, V>: HandleVoteEvent<TYPES, VOTE, CERT>,
{
    match collectors.entry(vote.view_number()) {
        Entry::Vacant(entry) => {
            tracing::debug!("Starting vote handle for view {:?}", vote.view_number());
            let info = AccumulatorInfo {
                public_key,
                membership: Arc::clone(membership),
                view: vote.view_number(),
                epoch,
                id,
            };
            let collector = create_vote_accumulator(
                &info,
                Arc::clone(event),
                event_stream,
                upgrade_lock.clone(),
            )
            .await?;

            entry.insert(collector);

            Ok(())
        }
        Entry::Occupied(mut entry) => {
            // handle the vote, and garbage collect if the vote collector is finished
            if entry
                .get_mut()
                .handle_vote_event(Arc::clone(event), event_stream)
                .await?
                .is_some()
            {
                entry.remove();
                *collectors = collectors.split_off(&vote.view_number());
            }

            Ok(())
        }
    }
}

/// Alias for Quorum vote accumulator
type QuorumVoteState<TYPES, V> =
    VoteCollectionTaskState<TYPES, QuorumVote<TYPES>, QuorumCertificate<TYPES>, V>;
/// Alias for DA vote accumulator
type DaVoteState<TYPES, V> = VoteCollectionTaskState<TYPES, DaVote<TYPES>, DaCertificate<TYPES>, V>;
/// Alias for Timeout vote accumulator
type TimeoutVoteState<TYPES, V> =
    VoteCollectionTaskState<TYPES, TimeoutVote<TYPES>, TimeoutCertificate<TYPES>, V>;
/// Alias for upgrade vote accumulator
type UpgradeVoteState<TYPES, V> =
    VoteCollectionTaskState<TYPES, UpgradeVote<TYPES>, UpgradeCertificate<TYPES>, V>;
/// Alias for View Sync Pre Commit vote accumulator
type ViewSyncPreCommitState<TYPES, V> = VoteCollectionTaskState<
    TYPES,
    ViewSyncPreCommitVote<TYPES>,
    ViewSyncPreCommitCertificate2<TYPES>,
    V,
>;
/// Alias for View Sync Commit vote accumulator
type ViewSyncCommitVoteState<TYPES, V> =
    VoteCollectionTaskState<TYPES, ViewSyncCommitVote<TYPES>, ViewSyncCommitCertificate2<TYPES>, V>;
/// Alias for View Sync Finalize vote accumulator
type ViewSyncFinalizeVoteState<TYPES, V> = VoteCollectionTaskState<
    TYPES,
    ViewSyncFinalizeVote<TYPES>,
    ViewSyncFinalizeCertificate2<TYPES>,
    V,
>;

impl<TYPES: NodeType> AggregatableVote<TYPES, QuorumVote<TYPES>, QuorumCertificate<TYPES>>
    for QuorumVote<TYPES>
{
    fn leader(
        &self,
        membership: &TYPES::Membership,
        epoch: TYPES::Epoch,
    ) -> Result<TYPES::SignatureKey> {
        membership.leader(self.view_number() + 1, epoch)
    }
    fn make_cert_event(
        certificate: QuorumCertificate<TYPES>,
        _key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::QcFormed(Left(certificate))
    }
}

impl<TYPES: NodeType> AggregatableVote<TYPES, UpgradeVote<TYPES>, UpgradeCertificate<TYPES>>
    for UpgradeVote<TYPES>
{
    fn leader(
        &self,
        membership: &TYPES::Membership,
        epoch: TYPES::Epoch,
    ) -> Result<TYPES::SignatureKey> {
        membership.leader(self.view_number(), epoch)
    }
    fn make_cert_event(
        certificate: UpgradeCertificate<TYPES>,
        _key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::UpgradeCertificateFormed(certificate)
    }
}

impl<TYPES: NodeType> AggregatableVote<TYPES, DaVote<TYPES>, DaCertificate<TYPES>>
    for DaVote<TYPES>
{
    fn leader(
        &self,
        membership: &TYPES::Membership,
        epoch: TYPES::Epoch,
    ) -> Result<TYPES::SignatureKey> {
        membership.leader(self.view_number(), epoch)
    }
    fn make_cert_event(
        certificate: DaCertificate<TYPES>,
        key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::DacSend(certificate, key.clone())
    }
}

impl<TYPES: NodeType> AggregatableVote<TYPES, TimeoutVote<TYPES>, TimeoutCertificate<TYPES>>
    for TimeoutVote<TYPES>
{
    fn leader(
        &self,
        membership: &TYPES::Membership,
        epoch: TYPES::Epoch,
    ) -> Result<TYPES::SignatureKey> {
        membership.leader(self.view_number() + 1, epoch)
    }
    fn make_cert_event(
        certificate: TimeoutCertificate<TYPES>,
        _key: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        HotShotEvent::QcFormed(Right(certificate))
    }
}

impl<TYPES: NodeType>
    AggregatableVote<TYPES, ViewSyncCommitVote<TYPES>, ViewSyncCommitCertificate2<TYPES>>
    for ViewSyncCommitVote<TYPES>
{
    fn leader(
        &self,
        membership: &TYPES::Membership,
        epoch: TYPES::Epoch,
    ) -> Result<TYPES::SignatureKey> {
        membership.leader(self.date().round + self.date().relay, epoch)
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
    fn leader(
        &self,
        membership: &TYPES::Membership,
        epoch: TYPES::Epoch,
    ) -> Result<TYPES::SignatureKey> {
        membership.leader(self.date().round + self.date().relay, epoch)
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
    fn leader(
        &self,
        membership: &TYPES::Membership,
        epoch: TYPES::Epoch,
    ) -> Result<TYPES::SignatureKey> {
        membership.leader(self.date().round + self.date().relay, epoch)
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
impl<TYPES: NodeType, V: Versions>
    HandleVoteEvent<TYPES, QuorumVote<TYPES>, QuorumCertificate<TYPES>>
    for QuorumVoteState<TYPES, V>
{
    async fn handle_vote_event(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<Option<QuorumCertificate<TYPES>>> {
        match event.as_ref() {
            HotShotEvent::QuorumVoteRecv(vote) => self.accumulate_vote(vote, sender).await,
            _ => Ok(None),
        }
    }
    fn filter(event: Arc<HotShotEvent<TYPES>>) -> bool {
        matches!(event.as_ref(), HotShotEvent::QuorumVoteRecv(_))
    }
}

// Handlers for all vote accumulators
#[async_trait]
impl<TYPES: NodeType, V: Versions>
    HandleVoteEvent<TYPES, UpgradeVote<TYPES>, UpgradeCertificate<TYPES>>
    for UpgradeVoteState<TYPES, V>
{
    async fn handle_vote_event(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<Option<UpgradeCertificate<TYPES>>> {
        match event.as_ref() {
            HotShotEvent::UpgradeVoteRecv(vote) => self.accumulate_vote(vote, sender).await,
            _ => Ok(None),
        }
    }
    fn filter(event: Arc<HotShotEvent<TYPES>>) -> bool {
        matches!(event.as_ref(), HotShotEvent::UpgradeVoteRecv(_))
    }
}

#[async_trait]
impl<TYPES: NodeType, V: Versions> HandleVoteEvent<TYPES, DaVote<TYPES>, DaCertificate<TYPES>>
    for DaVoteState<TYPES, V>
{
    async fn handle_vote_event(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<Option<DaCertificate<TYPES>>> {
        match event.as_ref() {
            HotShotEvent::DaVoteRecv(vote) => self.accumulate_vote(vote, sender).await,
            _ => Ok(None),
        }
    }
    fn filter(event: Arc<HotShotEvent<TYPES>>) -> bool {
        matches!(event.as_ref(), HotShotEvent::DaVoteRecv(_))
    }
}

#[async_trait]
impl<TYPES: NodeType, V: Versions>
    HandleVoteEvent<TYPES, TimeoutVote<TYPES>, TimeoutCertificate<TYPES>>
    for TimeoutVoteState<TYPES, V>
{
    async fn handle_vote_event(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<Option<TimeoutCertificate<TYPES>>> {
        match event.as_ref() {
            HotShotEvent::TimeoutVoteRecv(vote) => self.accumulate_vote(vote, sender).await,
            _ => Ok(None),
        }
    }
    fn filter(event: Arc<HotShotEvent<TYPES>>) -> bool {
        matches!(event.as_ref(), HotShotEvent::TimeoutVoteRecv(_))
    }
}

#[async_trait]
impl<TYPES: NodeType, V: Versions>
    HandleVoteEvent<TYPES, ViewSyncPreCommitVote<TYPES>, ViewSyncPreCommitCertificate2<TYPES>>
    for ViewSyncPreCommitState<TYPES, V>
{
    async fn handle_vote_event(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<Option<ViewSyncPreCommitCertificate2<TYPES>>> {
        match event.as_ref() {
            HotShotEvent::ViewSyncPreCommitVoteRecv(vote) => {
                self.accumulate_vote(vote, sender).await
            }
            _ => Ok(None),
        }
    }
    fn filter(event: Arc<HotShotEvent<TYPES>>) -> bool {
        matches!(event.as_ref(), HotShotEvent::ViewSyncPreCommitVoteRecv(_))
    }
}

#[async_trait]
impl<TYPES: NodeType, V: Versions>
    HandleVoteEvent<TYPES, ViewSyncCommitVote<TYPES>, ViewSyncCommitCertificate2<TYPES>>
    for ViewSyncCommitVoteState<TYPES, V>
{
    async fn handle_vote_event(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<Option<ViewSyncCommitCertificate2<TYPES>>> {
        match event.as_ref() {
            HotShotEvent::ViewSyncCommitVoteRecv(vote) => self.accumulate_vote(vote, sender).await,
            _ => Ok(None),
        }
    }
    fn filter(event: Arc<HotShotEvent<TYPES>>) -> bool {
        matches!(event.as_ref(), HotShotEvent::ViewSyncCommitVoteRecv(_))
    }
}

#[async_trait]
impl<TYPES: NodeType, V: Versions>
    HandleVoteEvent<TYPES, ViewSyncFinalizeVote<TYPES>, ViewSyncFinalizeCertificate2<TYPES>>
    for ViewSyncFinalizeVoteState<TYPES, V>
{
    async fn handle_vote_event(
        &mut self,
        event: Arc<HotShotEvent<TYPES>>,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> Result<Option<ViewSyncFinalizeCertificate2<TYPES>>> {
        match event.as_ref() {
            HotShotEvent::ViewSyncFinalizeVoteRecv(vote) => {
                self.accumulate_vote(vote, sender).await
            }
            _ => Ok(None),
        }
    }
    fn filter(event: Arc<HotShotEvent<TYPES>>) -> bool {
        matches!(event.as_ref(), HotShotEvent::ViewSyncFinalizeVoteRecv(_))
    }
}
