// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{
    cmp::max,
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use committable::Committable;
use futures::{FutureExt, Stream};
use hotshot::types::{BLSPubKey, SignatureKey, SystemContextHandle};
use hotshot_example_types::{
    block_types::{TestBlockHeader, TestBlockPayload, TestTransaction},
    node_types::{MemoryImpl, TestTypes, TestVersions},
    state_types::{TestInstanceState, TestValidatedState},
};
use hotshot_types::{
    data::{
        DaProposal2, EpochNumber, Leaf2, QuorumProposal2, VidDisperse, VidDisperseShare2,
        ViewChangeEvidence, ViewNumber,
    },
    drb::{INITIAL_DRB_RESULT, INITIAL_DRB_SEED_INPUT},
    message::{Proposal, UpgradeLock},
    simple_certificate::{
        DaCertificate2, QuorumCertificate2, TimeoutCertificate2, UpgradeCertificate,
        ViewSyncFinalizeCertificate2,
    },
    simple_vote::{
        DaData2, DaVote2, QuorumData2, QuorumVote2, TimeoutData2, TimeoutVote2,
        UpgradeProposalData, UpgradeVote, ViewSyncFinalizeData2, ViewSyncFinalizeVote2,
    },
    traits::{
        consensus_api::ConsensusApi,
        node_implementation::{ConsensusTime, NodeType},
        BlockPayload,
    },
};
use rand::{thread_rng, Rng};
use sha2::{Digest, Sha256};

use crate::helpers::{
    build_cert, build_da_certificate, build_vid_proposal, da_payload_commitment, key_pair_for_id,
};

#[derive(Clone)]
pub struct TestView {
    pub da_proposal: Proposal<TestTypes, DaProposal2<TestTypes>>,
    pub quorum_proposal: Proposal<TestTypes, QuorumProposal2<TestTypes>>,
    pub leaf: Leaf2<TestTypes>,
    pub view_number: ViewNumber,
    pub epoch_number: EpochNumber,
    pub membership: <TestTypes as NodeType>::Membership,
    pub vid_disperse: Proposal<TestTypes, VidDisperse<TestTypes>>,
    pub vid_proposal: (
        Vec<Proposal<TestTypes, VidDisperseShare2<TestTypes>>>,
        <TestTypes as NodeType>::SignatureKey,
    ),
    pub leader_public_key: <TestTypes as NodeType>::SignatureKey,
    pub da_certificate: DaCertificate2<TestTypes>,
    pub transactions: Vec<TestTransaction>,
    upgrade_data: Option<UpgradeProposalData<TestTypes>>,
    formed_upgrade_certificate: Option<UpgradeCertificate<TestTypes>>,
    view_sync_finalize_data: Option<ViewSyncFinalizeData2<TestTypes>>,
    timeout_cert_data: Option<TimeoutData2<TestTypes>>,
    upgrade_lock: UpgradeLock<TestTypes, TestVersions>,
}

impl TestView {
    pub async fn genesis(membership: &<TestTypes as NodeType>::Membership) -> Self {
        let genesis_view = ViewNumber::new(1);
        let genesis_epoch = EpochNumber::new(0);
        let upgrade_lock = UpgradeLock::new();

        let transactions = Vec::new();

        let (block_payload, metadata) =
            <TestBlockPayload as BlockPayload<TestTypes>>::from_transactions(
                transactions.clone(),
                &TestValidatedState::default(),
                &TestInstanceState::default(),
            )
            .await
            .unwrap();

        let builder_commitment = <TestBlockPayload as BlockPayload<TestTypes>>::builder_commitment(
            &block_payload,
            &metadata,
        );

        let (private_key, public_key) = key_pair_for_id::<TestTypes>(*genesis_view);

        let leader_public_key = public_key;

        let payload_commitment =
            da_payload_commitment::<TestTypes>(membership, transactions.clone(), genesis_epoch);

        let (vid_disperse, vid_proposal) = build_vid_proposal(
            membership,
            genesis_view,
            genesis_epoch,
            transactions.clone(),
            &private_key,
        );

        let da_certificate = build_da_certificate(
            membership,
            genesis_view,
            genesis_epoch,
            transactions.clone(),
            &public_key,
            &private_key,
            &upgrade_lock,
        )
        .await;

        let block_header = TestBlockHeader::new(
            &Leaf2::<TestTypes>::genesis(
                &TestValidatedState::default(),
                &TestInstanceState::default(),
            )
            .await,
            payload_commitment,
            builder_commitment,
            metadata,
        );

        let quorum_proposal_inner = QuorumProposal2::<TestTypes> {
            block_header: block_header.clone(),
            view_number: genesis_view,
            justify_qc: QuorumCertificate2::genesis::<TestVersions>(
                &TestValidatedState::default(),
                &TestInstanceState::default(),
            )
            .await,
            upgrade_certificate: None,
            view_change_evidence: None,
            drb_result: INITIAL_DRB_RESULT,
            drb_seed: INITIAL_DRB_SEED_INPUT,
        };

        let encoded_transactions = Arc::from(TestTransaction::encode(&transactions));
        let encoded_transactions_hash = Sha256::digest(&encoded_transactions);
        let block_payload_signature =
            <TestTypes as NodeType>::SignatureKey::sign(&private_key, &encoded_transactions_hash)
                .expect("Failed to sign block payload");

        let da_proposal_inner = DaProposal2::<TestTypes> {
            encoded_transactions: encoded_transactions.clone(),
            metadata,
            view_number: genesis_view,
            epoch: genesis_epoch,
        };

        let da_proposal = Proposal {
            data: da_proposal_inner,
            signature: block_payload_signature,
            _pd: PhantomData,
        };

        let mut leaf = Leaf2::from_quorum_proposal(&quorum_proposal_inner);
        leaf.fill_block_payload_unchecked(TestBlockPayload {
            transactions: transactions.clone(),
        });

        let signature = <BLSPubKey as SignatureKey>::sign(&private_key, leaf.commit().as_ref())
            .expect("Failed to sign leaf commitment!");

        let quorum_proposal = Proposal {
            data: quorum_proposal_inner,
            signature,
            _pd: PhantomData,
        };

        TestView {
            quorum_proposal,
            leaf,
            view_number: genesis_view,
            epoch_number: genesis_epoch,
            membership: membership.clone(),
            vid_disperse,
            vid_proposal: (vid_proposal, public_key),
            da_certificate,
            transactions,
            leader_public_key,
            upgrade_data: None,
            formed_upgrade_certificate: None,
            view_sync_finalize_data: None,
            timeout_cert_data: None,
            da_proposal,
            upgrade_lock,
        }
    }

    /// Moves the generator to the next view by referencing an ancestor. To have a standard,
    /// sequentially ordered set of generated test views, use the `next_view` function. Otherwise,
    /// this method can be used to start from an ancestor (whose view is at least one view older
    /// than the current view) and construct valid views without the data structures in the task
    /// failing by expecting views that they has never seen.
    pub async fn next_view_from_ancestor(&self, ancestor: TestView) -> Self {
        let old = ancestor;
        let old_view = old.view_number;
        let old_epoch = old.epoch_number;

        // This ensures that we're always moving forward in time since someone could pass in any
        // test view here.
        let next_view = max(old_view, self.view_number) + 1;

        let membership = &self.membership;

        let transactions = &self.transactions;

        let quorum_data = QuorumData2 {
            leaf_commit: old.leaf.commit(),
            epoch: old_epoch,
        };

        let (old_private_key, old_public_key) = key_pair_for_id::<TestTypes>(*old_view);

        let (private_key, public_key) = key_pair_for_id::<TestTypes>(*next_view);

        let leader_public_key = public_key;

        let (block_payload, metadata) =
            <TestBlockPayload as BlockPayload<TestTypes>>::from_transactions(
                transactions.clone(),
                &TestValidatedState::default(),
                &TestInstanceState::default(),
            )
            .await
            .unwrap();
        let builder_commitment = <TestBlockPayload as BlockPayload<TestTypes>>::builder_commitment(
            &block_payload,
            &metadata,
        );

        let payload_commitment =
            da_payload_commitment::<TestTypes>(membership, transactions.clone(), self.epoch_number);

        let (vid_disperse, vid_proposal) = build_vid_proposal(
            membership,
            next_view,
            self.epoch_number,
            transactions.clone(),
            &private_key,
        );

        let da_certificate = build_da_certificate::<TestTypes, TestVersions>(
            membership,
            next_view,
            self.epoch_number,
            transactions.clone(),
            &public_key,
            &private_key,
            &self.upgrade_lock,
        )
        .await;

        let quorum_certificate = build_cert::<
            TestTypes,
            TestVersions,
            QuorumData2<TestTypes>,
            QuorumVote2<TestTypes>,
            QuorumCertificate2<TestTypes>,
        >(
            quorum_data,
            membership,
            old_view,
            self.epoch_number,
            &old_public_key,
            &old_private_key,
            &self.upgrade_lock,
        )
        .await;

        let upgrade_certificate = if let Some(ref data) = self.upgrade_data {
            let cert = build_cert::<
                TestTypes,
                TestVersions,
                UpgradeProposalData<TestTypes>,
                UpgradeVote<TestTypes>,
                UpgradeCertificate<TestTypes>,
            >(
                data.clone(),
                membership,
                next_view,
                self.epoch_number,
                &public_key,
                &private_key,
                &self.upgrade_lock,
            )
            .await;

            Some(cert)
        } else {
            self.formed_upgrade_certificate.clone()
        };

        let view_sync_certificate = if let Some(ref data) = self.view_sync_finalize_data {
            let cert = build_cert::<
                TestTypes,
                TestVersions,
                ViewSyncFinalizeData2<TestTypes>,
                ViewSyncFinalizeVote2<TestTypes>,
                ViewSyncFinalizeCertificate2<TestTypes>,
            >(
                data.clone(),
                membership,
                next_view,
                self.epoch_number,
                &public_key,
                &private_key,
                &self.upgrade_lock,
            )
            .await;

            Some(cert)
        } else {
            None
        };

        let timeout_certificate = if let Some(ref data) = self.timeout_cert_data {
            let cert = build_cert::<
                TestTypes,
                TestVersions,
                TimeoutData2<TestTypes>,
                TimeoutVote2<TestTypes>,
                TimeoutCertificate2<TestTypes>,
            >(
                data.clone(),
                membership,
                next_view,
                self.epoch_number,
                &public_key,
                &private_key,
                &self.upgrade_lock,
            )
            .await;

            Some(cert)
        } else {
            None
        };

        let view_change_evidence = if let Some(tc) = timeout_certificate {
            Some(ViewChangeEvidence::Timeout(tc))
        } else {
            view_sync_certificate.map(ViewChangeEvidence::ViewSync)
        };

        let random = thread_rng().gen_range(0..=u64::MAX);

        let block_header = TestBlockHeader {
            block_number: *next_view,
            timestamp: *next_view,
            payload_commitment,
            builder_commitment,
            metadata,
            random,
        };

        let proposal = QuorumProposal2::<TestTypes> {
            block_header: block_header.clone(),
            view_number: next_view,
            justify_qc: quorum_certificate.clone(),
            upgrade_certificate: upgrade_certificate.clone(),
            view_change_evidence,
            drb_result: INITIAL_DRB_RESULT,
            drb_seed: INITIAL_DRB_SEED_INPUT,
        };

        let mut leaf = Leaf2::from_quorum_proposal(&proposal);
        leaf.fill_block_payload_unchecked(TestBlockPayload {
            transactions: transactions.clone(),
        });

        let signature = <BLSPubKey as SignatureKey>::sign(&private_key, leaf.commit().as_ref())
            .expect("Failed to sign leaf commitment.");

        let quorum_proposal = Proposal {
            data: proposal,
            signature,
            _pd: PhantomData,
        };

        let encoded_transactions = Arc::from(TestTransaction::encode(transactions));
        let encoded_transactions_hash = Sha256::digest(&encoded_transactions);
        let block_payload_signature =
            <TestTypes as NodeType>::SignatureKey::sign(&private_key, &encoded_transactions_hash)
                .expect("Failed to sign block payload");

        let da_proposal_inner = DaProposal2::<TestTypes> {
            encoded_transactions: encoded_transactions.clone(),
            metadata,
            view_number: next_view,
            epoch: old_epoch,
        };

        let da_proposal = Proposal {
            data: da_proposal_inner,
            signature: block_payload_signature,
            _pd: PhantomData,
        };

        let upgrade_lock = UpgradeLock::new();

        TestView {
            quorum_proposal,
            leaf,
            view_number: next_view,
            epoch_number: self.epoch_number,
            membership: self.membership.clone(),
            vid_disperse,
            vid_proposal: (vid_proposal, public_key),
            da_certificate,
            leader_public_key,
            // Transactions and upgrade data need to be manually injected each view,
            // so we reset for the next view.
            transactions: Vec::new(),
            upgrade_data: None,
            // We preserve the upgrade_certificate once formed,
            // and reattach it on every future view until cleared.
            formed_upgrade_certificate: upgrade_certificate,
            view_sync_finalize_data: None,
            timeout_cert_data: None,
            da_proposal,
            upgrade_lock,
        }
    }

    pub async fn next_view(&self) -> Self {
        self.next_view_from_ancestor(self.clone()).await
    }

    pub async fn create_quorum_vote(
        &self,
        handle: &SystemContextHandle<TestTypes, MemoryImpl, TestVersions>,
    ) -> QuorumVote2<TestTypes> {
        QuorumVote2::<TestTypes>::create_signed_vote(
            QuorumData2 {
                leaf_commit: self.leaf.commit(),
                epoch: self.epoch_number,
            },
            self.view_number,
            &handle.public_key(),
            handle.private_key(),
            &handle.hotshot.upgrade_lock,
        )
        .await
        .expect("Failed to generate a signature on QuorumVote")
    }

    pub async fn create_upgrade_vote(
        &self,
        data: UpgradeProposalData<TestTypes>,
        handle: &SystemContextHandle<TestTypes, MemoryImpl, TestVersions>,
    ) -> UpgradeVote<TestTypes> {
        UpgradeVote::<TestTypes>::create_signed_vote(
            data,
            self.view_number,
            &handle.public_key(),
            handle.private_key(),
            &handle.hotshot.upgrade_lock,
        )
        .await
        .expect("Failed to generate a signature on UpgradVote")
    }

    pub async fn create_da_vote(
        &self,
        data: DaData2<TestTypes>,
        handle: &SystemContextHandle<TestTypes, MemoryImpl, TestVersions>,
    ) -> DaVote2<TestTypes> {
        DaVote2::create_signed_vote(
            data,
            self.view_number,
            &handle.public_key(),
            handle.private_key(),
            &handle.hotshot.upgrade_lock,
        )
        .await
        .expect("Failed to sign DaData")
    }
}

pub struct TestViewGenerator {
    pub current_view: Option<TestView>,
    pub membership: <TestTypes as NodeType>::Membership,
}

impl TestViewGenerator {
    pub fn generate(membership: <TestTypes as NodeType>::Membership) -> Self {
        TestViewGenerator {
            current_view: None,
            membership,
        }
    }

    pub fn add_upgrade(&mut self, upgrade_proposal_data: UpgradeProposalData<TestTypes>) {
        if let Some(ref view) = self.current_view {
            self.current_view = Some(TestView {
                upgrade_data: Some(upgrade_proposal_data),
                ..view.clone()
            });
        } else {
            tracing::error!("Cannot attach upgrade proposal to the genesis view.");
        }
    }

    pub fn add_transactions(&mut self, transactions: Vec<TestTransaction>) {
        if let Some(ref view) = self.current_view {
            self.current_view = Some(TestView {
                transactions,
                ..view.clone()
            });
        } else {
            tracing::error!("Cannot attach transactions to the genesis view.");
        }
    }

    pub fn add_view_sync_finalize(
        &mut self,
        view_sync_finalize_data: ViewSyncFinalizeData2<TestTypes>,
    ) {
        if let Some(ref view) = self.current_view {
            self.current_view = Some(TestView {
                view_sync_finalize_data: Some(view_sync_finalize_data),
                ..view.clone()
            });
        } else {
            tracing::error!("Cannot attach view sync finalize to the genesis view.");
        }
    }

    pub fn add_timeout(&mut self, timeout_data: TimeoutData2<TestTypes>) {
        if let Some(ref view) = self.current_view {
            self.current_view = Some(TestView {
                timeout_cert_data: Some(timeout_data),
                ..view.clone()
            });
        } else {
            tracing::error!("Cannot attach timeout cert to the genesis view.")
        }
    }

    /// Advances to the next view by skipping the current view and not adding it to the state tree.
    /// This is useful when simulating that a timeout has occurred.
    pub fn advance_view_number_by(&mut self, n: u64) {
        if let Some(ref view) = self.current_view {
            self.current_view = Some(TestView {
                view_number: view.view_number + n,
                ..view.clone()
            })
        } else {
            tracing::error!("Cannot attach view sync finalize to the genesis view.");
        }
    }

    pub async fn next_from_ancestor_view(&mut self, ancestor: TestView) {
        if let Some(ref view) = self.current_view {
            self.current_view = Some(view.next_view_from_ancestor(ancestor).await)
        } else {
            tracing::error!("Cannot attach ancestor to genesis view.");
        }
    }
}

impl Stream for TestViewGenerator {
    type Item = TestView;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mem = &self.membership.clone();
        let curr_view = &self.current_view.clone();

        let mut fut = if let Some(ref view) = curr_view {
            async move { TestView::next_view(view).await }.boxed()
        } else {
            async move { TestView::genesis(mem).await }.boxed()
        };

        match fut.as_mut().poll(cx) {
            Poll::Ready(test_view) => {
                self.current_view = Some(test_view.clone());
                Poll::Ready(Some(test_view))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}
