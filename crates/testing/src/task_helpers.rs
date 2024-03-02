#![allow(clippy::panic)]
use std::marker::PhantomData;

use hotshot_example_types::{
    block_types::{TestBlockHeader, TestBlockPayload, TestTransaction},
    node_types::{MemoryImpl, TestTypes},
    state_types::{TestInstanceState, TestValidatedState},
};

use crate::test_builder::TestMetadata;
use commit::Committable;
use ethereum_types::U256;
use hotshot::{
    types::{BLSPubKey, SignatureKey, SystemContextHandle},
    HotShotInitializer, Memberships, Networks, SystemContext,
};
use hotshot_task_impls::events::HotShotEvent;
use hotshot_types::{
    consensus::ConsensusMetricsValue,
    data::{Leaf, QuorumProposal, VidDisperse, VidScheme, VidSchemeTrait, ViewNumber},
    message::{GeneralConsensusMessage, Proposal},
    simple_certificate::{DACertificate, QuorumCertificate, UpgradeCertificate},
    simple_vote::{DAData, DAVote, SimpleVote, UpgradeProposalData, UpgradeVote},
    traits::{
        block_contents::{vid_commitment, BlockHeader, TestableBlock},
        consensus_api::ConsensusApi,
        election::Membership,
        node_implementation::{ConsensusTime, NodeType},
        states::ValidatedState,
        BlockPayload,
    },
    vote::HasViewNumber,
};

use async_broadcast::{Receiver, Sender};
use async_lock::RwLockUpgradableReadGuard;
use bitvec::bitvec;
use hotshot_types::simple_vote::QuorumData;
use hotshot_types::simple_vote::QuorumVote;
use hotshot_types::utils::View;
use hotshot_types::utils::ViewInner;
use hotshot_types::vote::Certificate;
use hotshot_types::vote::Vote;

use serde::Serialize;
use std::{fmt::Debug, hash::Hash, sync::Arc};

/// create the [`SystemContextHandle`] from a node id
/// # Panics
/// if cannot create a [`HotShotInitializer`]
pub async fn build_system_handle(
    node_id: u64,
) -> (
    SystemContextHandle<TestTypes, MemoryImpl>,
    Sender<HotShotEvent<TestTypes>>,
    Receiver<HotShotEvent<TestTypes>>,
) {
    let builder = TestMetadata::default_multiple_rounds();

    let launcher = builder.gen_launcher::<TestTypes, MemoryImpl>(node_id);

    let networks = (launcher.resource_generator.channel_generator)(node_id);
    let storage = (launcher.resource_generator.storage)(node_id);
    let config = launcher.resource_generator.config.clone();

    let initializer = HotShotInitializer::<TestTypes>::from_genesis(&TestInstanceState {}).unwrap();

    let known_nodes_with_stake = config.known_nodes_with_stake.clone();
    let private_key = config.my_own_validator_config.private_key.clone();
    let public_key = config.my_own_validator_config.public_key;
    let quorum_election_config =
        config.election_config.clone().unwrap_or_else(|| {
            <TestTypes as NodeType>::Membership::default_election_config(
                config.total_nodes.get() as u64
            )
        });

    let committee_election_config =
        config.election_config.clone().unwrap_or_else(|| {
            <TestTypes as NodeType>::Membership::default_election_config(
                config.total_nodes.get() as u64
            )
        });
    let networks_bundle = Networks {
        quorum_network: networks.0.clone(),
        da_network: networks.1.clone(),
        _pd: PhantomData,
    };

    let memberships = Memberships {
        quorum_membership: <TestTypes as NodeType>::Membership::create_election(
            known_nodes_with_stake.clone(),
            quorum_election_config.clone(),
        ),
        da_membership: <TestTypes as NodeType>::Membership::create_election(
            known_nodes_with_stake.clone(),
            committee_election_config,
        ),
        vid_membership: <TestTypes as NodeType>::Membership::create_election(
            known_nodes_with_stake.clone(),
            quorum_election_config.clone(),
        ),
        view_sync_membership: <TestTypes as NodeType>::Membership::create_election(
            known_nodes_with_stake.clone(),
            quorum_election_config,
        ),
    };

    SystemContext::init(
        public_key,
        private_key,
        node_id,
        config,
        storage,
        memberships,
        networks_bundle,
        initializer,
        ConsensusMetricsValue::default(),
    )
    .await
    .expect("Could not init hotshot")
}

/// create certificate
/// # Panics
/// if we fail to sign the data
pub fn build_cert<
    TYPES: NodeType<SignatureKey = BLSPubKey>,
    DATAType: Committable + Clone + Eq + Hash + Serialize + Debug + 'static,
    VOTE: Vote<TYPES, Commitment = DATAType>,
    CERT: Certificate<TYPES, Voteable = VOTE::Commitment>,
>(
    data: DATAType,
    membership: &TYPES::Membership,
    view: TYPES::Time,
    public_key: &TYPES::SignatureKey,
    private_key: &<TYPES::SignatureKey as SignatureKey>::PrivateKey,
) -> CERT {
    let real_qc_sig = build_assembled_sig::<TYPES, VOTE, CERT, DATAType>(&data, membership, view);

    let vote =
        SimpleVote::<TYPES, DATAType>::create_signed_vote(data, view, public_key, private_key)
            .expect("Failed to sign data!");
    let cert = CERT::create_signed_certificate(
        vote.get_data_commitment(),
        vote.get_data().clone(),
        real_qc_sig,
        vote.get_view_number(),
    );
    cert
}

/// create signature
/// # Panics
/// if fails to convert node id into keypair
pub fn build_assembled_sig<
    TYPES: NodeType<SignatureKey = BLSPubKey>,
    VOTE: Vote<TYPES>,
    CERT: Certificate<TYPES, Voteable = VOTE::Commitment>,
    DATAType: Committable + Clone + Eq + Hash + Serialize + Debug + 'static,
>(
    data: &DATAType,
    membership: &TYPES::Membership,
    view: TYPES::Time,
) -> <TYPES::SignatureKey as SignatureKey>::QCType {
    let stake_table = membership.get_committee_qc_stake_table();
    let real_qc_pp: <TYPES::SignatureKey as SignatureKey>::QCParams =
        <TYPES::SignatureKey as SignatureKey>::get_public_parameter(
            stake_table.clone(),
            U256::from(CERT::threshold(membership)),
        );
    let total_nodes = stake_table.len();
    let signers = bitvec![1; total_nodes];
    let mut sig_lists = Vec::new();

    // assemble the vote
    for node_id in 0..total_nodes {
        let (private_key_i, public_key_i) = key_pair_for_id(node_id.try_into().unwrap());
        let vote: SimpleVote<TYPES, DATAType> = SimpleVote::<TYPES, DATAType>::create_signed_vote(
            data.clone(),
            view,
            &public_key_i,
            &private_key_i,
        )
        .expect("Failed to sign data!");
        let original_signature: <TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType =
            vote.get_signature();
        sig_lists.push(original_signature);
    }

    let real_qc_sig = <TYPES::SignatureKey as SignatureKey>::assemble(
        &real_qc_pp,
        signers.as_bitslice(),
        &sig_lists[..],
    );

    real_qc_sig
}

/// build a quorum proposal and signature
#[allow(clippy::too_many_lines)]
async fn build_quorum_proposal_and_signature(
    handle: &SystemContextHandle<TestTypes, MemoryImpl>,
    upgrade_certificate: Option<UpgradeCertificate<TestTypes>>,
    private_key: &<BLSPubKey as SignatureKey>::PrivateKey,
    public_key: &BLSPubKey,
    view: u64,
) -> (
    QuorumProposal<TestTypes>,
    <BLSPubKey as SignatureKey>::PureAssembledSignatureType,
) {
    // build the genesis view
    let genesis_consensus = handle.get_consensus();
    let cur_consensus = genesis_consensus.upgradable_read().await;
    let mut consensus = RwLockUpgradableReadGuard::upgrade(cur_consensus).await;
    // parent_view_number should be equal to 0
    let parent_view_number = &consensus.high_qc.get_view_number();
    assert_eq!(parent_view_number.get_u64(), 0);
    let Some(parent_view) = consensus.validated_state_map.get(parent_view_number) else {
        panic!("Couldn't find high QC parent in state map.");
    };
    let Some(leaf_view_0) = parent_view.get_leaf_commitment() else {
        panic!("Parent of high QC points to a view without a proposal");
    };
    let Some(leaf_view_0) = consensus.saved_leaves.get(&leaf_view_0) else {
        panic!("Failed to find high QC parent.");
    };
    let parent_leaf = leaf_view_0.clone();

    // every event input is seen on the event stream in the output.
    let block = <TestBlockPayload as TestableBlock>::genesis();
    let payload_commitment = vid_commitment(
        &block.encode().unwrap().collect(),
        handle.hotshot.memberships.quorum_membership.total_nodes(),
    );
    let mut parent_state = Arc::new(<TestValidatedState as ValidatedState>::from_header(
        &parent_leaf.block_header,
    ));
    let block_header = TestBlockHeader::new(
        &*parent_state,
        &TestInstanceState {},
        &parent_leaf.block_header,
        payload_commitment,
        (),
    );
    // current leaf that can be re-assigned everytime when entering a new view
    let mut leaf = Leaf {
        view_number: ViewNumber::new(1),
        justify_qc: consensus.high_qc.clone(),
        parent_commitment: parent_leaf.commit(),
        block_header: block_header.clone(),
        block_payload: None,
        proposer_id: *handle.public_key(),
    };

    let mut signature = <BLSPubKey as SignatureKey>::sign(private_key, leaf.commit().as_ref())
        .expect("Failed to sign leaf commitment!");
    let mut proposal = QuorumProposal::<TestTypes> {
        block_header: block_header.clone(),
        view_number: ViewNumber::new(1),
        justify_qc: QuorumCertificate::genesis(),
        timeout_certificate: None,
        upgrade_certificate: None,
        proposer_id: leaf.proposer_id,
    };

    // Only view 2 is tested, higher views are not tested
    for cur_view in 2..=view {
        let state_new_view = Arc::new(
            parent_state
                .validate_and_apply_header(&TestInstanceState {}, &block_header, &block_header)
                .unwrap(),
        );
        // save states for the previous view to pass all the qc checks
        // In the long term, we want to get rid of this, do not manually update consensus state
        consensus.validated_state_map.insert(
            ViewNumber::new(cur_view - 1),
            View {
                view_inner: ViewInner::Leaf {
                    leaf: leaf.commit(),
                    state: state_new_view.clone(),
                },
            },
        );
        consensus.saved_leaves.insert(leaf.commit(), leaf.clone());
        // create a qc by aggregate signatures on the previous view (the data signed is last leaf commitment)
        let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
        let quorum_data = QuorumData {
            leaf_commit: leaf.commit(),
        };
        let created_qc = build_cert::<
            TestTypes,
            QuorumData<TestTypes>,
            QuorumVote<TestTypes>,
            QuorumCertificate<TestTypes>,
        >(
            quorum_data,
            &quorum_membership,
            ViewNumber::new(cur_view - 1),
            public_key,
            private_key,
        );
        // create a new leaf for the current view
        let parent_leaf = leaf.clone();
        let leaf_new_view = Leaf {
            view_number: ViewNumber::new(cur_view),
            justify_qc: created_qc.clone(),
            parent_commitment: parent_leaf.commit(),
            block_header: block_header.clone(),
            block_payload: None,
            proposer_id: quorum_membership.get_leader(ViewNumber::new(cur_view)),
        };
        let signature_new_view =
            <BLSPubKey as SignatureKey>::sign(private_key, leaf_new_view.commit().as_ref())
                .expect("Failed to sign leaf commitment!");

        // We only attach the upgrade_certificate on the requested view.
        let upgrade_cert = if cur_view == view {
            upgrade_certificate.clone()
        } else {
            None
        };

        let proposal_new_view = QuorumProposal::<TestTypes> {
            block_header: block_header.clone(),
            view_number: ViewNumber::new(cur_view),
            justify_qc: created_qc,
            timeout_certificate: None,
            upgrade_certificate: upgrade_cert,
            proposer_id: leaf_new_view.clone().proposer_id,
        };
        proposal = proposal_new_view;
        signature = signature_new_view;
        leaf = leaf_new_view;
        parent_state = state_new_view;
    }

    (proposal, signature)
}

pub struct MessagesGenerator {
    pub current_messages: Option<Messages>,
    pub quorum_membership: <TestTypes as NodeType>::Membership,
}

#[derive(Clone)]
pub struct Messages {
    pub quorum_proposal: Proposal<TestTypes, QuorumProposal<TestTypes>>,
    pub leaf: Leaf<TestTypes>,
    pub view: ViewNumber,
    pub quorum_membership: <TestTypes as NodeType>::Membership,
}

impl Iterator for MessagesGenerator {
    type Item = Messages;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(messages) = &self.current_messages {
            self.current_messages = Some(Messages::next_view(&messages));
        } else {
            self.current_messages = Some(Messages::genesis(&self.quorum_membership));
        }

        self.current_messages.clone()
    }
}

impl MessagesGenerator {
    pub fn new(quorum_membership: <TestTypes as NodeType>::Membership) -> Self {
        MessagesGenerator {
            current_messages: None,
            quorum_membership,
        }
    }
}

impl Messages {
    pub fn genesis(quorum_membership: &<TestTypes as NodeType>::Membership) -> Self {
        let genesis_view = ViewNumber::new(1);

        let (private_key, public_key) = key_pair_for_id(*genesis_view);

        let payload_commitment = vid_commitment(&Vec::new(), quorum_membership.total_nodes());

        let block_header = TestBlockHeader {
            block_number: 1,
            payload_commitment,
        };

        let proposal = QuorumProposal::<TestTypes> {
            block_header: block_header.clone(),
            view_number: genesis_view,
            justify_qc: QuorumCertificate::genesis(),
            timeout_certificate: None,
            upgrade_certificate: None,
            proposer_id: public_key,
        };

        let leaf = Leaf {
            view_number: genesis_view,
            justify_qc: QuorumCertificate::genesis(),
            parent_commitment: Leaf::genesis(&TestInstanceState {}).commit(),
            block_header: block_header.clone(),
            block_payload: None,
            proposer_id: public_key,
        };

        let signature = <BLSPubKey as SignatureKey>::sign(&private_key, leaf.commit().as_ref())
            .expect("Failed to sign leaf commitment!");

        let quorum_proposal = Proposal {
            data: proposal,
            signature,
            _pd: PhantomData,
        };

        Messages {
            quorum_proposal,
            leaf,
            view: genesis_view,
            quorum_membership: quorum_membership.clone(),
        }
    }

    pub fn next_view(&self) -> Self {
        let old = self;

        let view = old.view + 1;

        let quorum_data = QuorumData {
            leaf_commit: old.leaf.commit(),
        };

        let (old_private_key, old_public_key) = key_pair_for_id(*old.view);

        let (private_key, public_key) = key_pair_for_id(*view);

        let quorum_certificate = build_cert::<
            TestTypes,
            QuorumData<TestTypes>,
            QuorumVote<TestTypes>,
            QuorumCertificate<TestTypes>,
        >(
            quorum_data,
            &old.quorum_membership,
            old.view,
            &old_public_key,
            &old_private_key,
        );

        let leaf = Leaf {
            view_number: view,
            justify_qc: quorum_certificate.clone(),
            parent_commitment: old.leaf.commit(),
            block_header: old.quorum_proposal.data.block_header.clone(),
            block_payload: None,
            proposer_id: public_key,
        };

        let signature = <BLSPubKey as SignatureKey>::sign(&private_key, leaf.commit().as_ref())
            .expect("Failed to sign leaf commitment.");

        let payload_commitment = vid_commitment(&Vec::new(), self.quorum_membership.total_nodes());

        let block_header = TestBlockHeader {
            block_number: 1,
            payload_commitment,
        };

        let proposal = QuorumProposal::<TestTypes> {
            block_header: block_header.clone(),
            view_number: view,
            justify_qc: quorum_certificate.clone(),
            timeout_certificate: None,
            upgrade_certificate: None,
            proposer_id: leaf.clone().proposer_id,
        };

        let quorum_proposal = Proposal {
            data: proposal,
            signature,
            _pd: PhantomData,
        };

        Messages {
            quorum_proposal,
            leaf,
            view,
            quorum_membership: old.quorum_membership.clone(),
        }
    }

    pub fn create_da_certificate(&self) -> DACertificate<TestTypes> {
        let (private_key, public_key) = key_pair_for_id(*self.view);

        build_da_certificate(
            &self.quorum_membership,
            self.view,
            &public_key,
            &private_key,
        )
    }

    pub fn create_vid_proposal(
        &self,
    ) -> (
        Proposal<TestTypes, VidDisperse<TestTypes>>,
        <TestTypes as NodeType>::SignatureKey,
    ) {
        let (private_key, public_key) = key_pair_for_id(*self.view);

        let vid_proposal = build_vid_proposal(&self.quorum_membership, self.view, &private_key);

        (vid_proposal, public_key)
    }

    pub fn create_vote(
        &self,
        handle: &SystemContextHandle<TestTypes, MemoryImpl>,
    ) -> QuorumVote<TestTypes> {
        QuorumVote::<TestTypes>::create_signed_vote(
            QuorumData {
                leaf_commit: self.leaf.commit(),
            },
            self.view,
            &handle.public_key(),
            &handle.private_key(),
        )
        .expect("Failed to generate a signature on QuorumVote")
    }
}

/// build a quorum proposal and signature
#[allow(clippy::too_many_lines)]
pub async fn build_quorum_proposals_with_upgrade(
    handle: &SystemContextHandle<TestTypes, MemoryImpl>,
    upgrade_data: Option<UpgradeProposalData<TestTypes>>,
    public_key: &BLSPubKey,
    view: u64,
    count: u64,
) -> (
    Vec<Proposal<TestTypes, QuorumProposal<TestTypes>>>,
    Vec<QuorumVote<TestTypes>>,
) {
    let mut proposals = Vec::new();
    let mut votes = Vec::new();
    // build the genesis view
    let genesis_consensus = handle.get_consensus();
    // let cur_consensus = genesis_consensus.upgradable_read().await;
    let mut consensus = genesis_consensus.write().await; // RwLockUpgradableReadGuard::upgrade(cur_consensus).await;
                                                         // parent_view_number should be equal to 0
    let parent_view_number = &consensus.high_qc.get_view_number();
    assert_eq!(parent_view_number.get_u64(), 0);
    let Some(parent_view) = consensus.validated_state_map.get(parent_view_number) else {
        panic!("Couldn't find high QC parent in state map.");
    };
    let Some(leaf_view_0) = parent_view.get_leaf_commitment() else {
        panic!("Parent of high QC points to a view without a proposal");
    };
    println!("leaf commitment {:?}", leaf_view_0);
    let Some(leaf_view_0) = consensus.saved_leaves.get(&leaf_view_0) else {
        panic!("Failed to find high QC parent.");
    };
    let parent_leaf = leaf_view_0.clone();
    println!("parent leaf {:?}", parent_leaf);
    assert_eq!(parent_leaf, Leaf::genesis(&TestInstanceState {}));

    // every event input is seen on the event stream in the output.
    let block = <TestBlockPayload as TestableBlock>::genesis();
    let payload_commitment = vid_commitment(
        &block.encode().unwrap().collect(),
        handle.hotshot.memberships.quorum_membership.total_nodes(),
    );
    let mut parent_state = Arc::new(<TestValidatedState as ValidatedState>::from_header(
        &parent_leaf.block_header,
    ));
    assert_eq!(
        parent_leaf.block_header,
        TestBlockHeader::genesis(&TestInstanceState {}).0
    );
    let block_header = TestBlockHeader::new(
        &*parent_state,
        &TestInstanceState {},
        &parent_leaf.block_header,
        payload_commitment,
        (),
    );
    // current leaf that can be re-assigned everytime when entering a new view
    let mut leaf = Leaf {
        view_number: ViewNumber::new(1),
        justify_qc: consensus.high_qc.clone(),
        parent_commitment: parent_leaf.commit(),
        block_header: block_header.clone(),
        block_payload: None,
        proposer_id: *handle.public_key(),
    };

    let (private_key, public_key) = key_pair_for_id(1);
    let mut signature = <BLSPubKey as SignatureKey>::sign(&private_key, leaf.commit().as_ref())
        .expect("Failed to sign leaf commitment!");
    let mut proposal = QuorumProposal::<TestTypes> {
        block_header: block_header.clone(),
        view_number: ViewNumber::new(1),
        justify_qc: QuorumCertificate::genesis(),
        timeout_certificate: None,
        upgrade_certificate: None,
        proposer_id: leaf.proposer_id,
    };
    let vote = QuorumVote::<TestTypes>::create_signed_vote(
        QuorumData {
            leaf_commit: leaf.commit(),
        },
        ViewNumber::new(1),
        &public_key,
        &private_key,
    )
    .unwrap();
    votes.push(vote);

    proposals.push(Proposal {
        data: proposal,
        signature,
        _pd: PhantomData,
    });

    // Only view 2 is tested, higher views are not tested
    for cur_view in 2..=(view + count) {
        let (private_key, public_key) = key_pair_for_id(cur_view);
        let state_new_view = Arc::new(
            parent_state
                .validate_and_apply_header(&TestInstanceState {}, &block_header, &block_header)
                .unwrap(),
        );
        // save states for the previous view to pass all the qc checks
        // In the long term, we want to get rid of this, do not manually update consensus state
        // consensus.validated_state_map.insert(
        //     ViewNumber::new(cur_view - 1),
        //     View {
        //         view_inner: ViewInner::Leaf {
        //             leaf: leaf.commit(),
        //             state: state_new_view.clone(),
        //         },
        //     },
        // );
        // consensus.saved_leaves.insert(leaf.commit(), leaf.clone());
        // create a qc by aggregate signatures on the previous view (the data signed is last leaf commitment)
        let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
        let quorum_data = QuorumData {
            leaf_commit: leaf.commit(),
        };
        let created_qc = build_cert::<
            TestTypes,
            QuorumData<TestTypes>,
            QuorumVote<TestTypes>,
            QuorumCertificate<TestTypes>,
        >(
            quorum_data,
            &quorum_membership,
            ViewNumber::new(cur_view - 1),
            &public_key,
            &private_key,
        );
        // create a new leaf for the current view
        let parent_leaf = leaf.clone();
        let leaf_new_view = Leaf {
            view_number: ViewNumber::new(cur_view),
            justify_qc: created_qc.clone(),
            parent_commitment: parent_leaf.commit(),
            block_header: block_header.clone(),
            block_payload: None,
            proposer_id: quorum_membership.get_leader(ViewNumber::new(cur_view)),
        };
        let signature_new_view =
            <BLSPubKey as SignatureKey>::sign(&private_key, leaf_new_view.commit().as_ref())
                .expect("Failed to sign leaf commitment!");

        let upgrade_certificate = if cur_view != view {
            None
        } else if let Some(ref data) = upgrade_data {
            let cert = build_cert::<
                TestTypes,
                UpgradeProposalData<TestTypes>,
                UpgradeVote<TestTypes>,
                UpgradeCertificate<TestTypes>,
            >(
                data.clone(),
                &quorum_membership,
                ViewNumber::new(cur_view),
                &public_key,
                &private_key,
            );

            Some(cert)
        } else {
            None
        };

        let proposal_new_view = QuorumProposal::<TestTypes> {
            block_header: block_header.clone(),
            view_number: ViewNumber::new(cur_view),
            justify_qc: created_qc,
            timeout_certificate: None,
            upgrade_certificate,
            proposer_id: leaf_new_view.clone().proposer_id,
        };
        proposal = proposal_new_view;
        let vote = QuorumVote::<TestTypes>::create_signed_vote(
            QuorumData {
                leaf_commit: leaf.commit(),
            },
            ViewNumber::new(cur_view),
            handle.public_key(),
            handle.private_key(),
        )
        .unwrap();
        votes.push(vote);
        signature = signature_new_view;
        proposals.push(Proposal {
            data: proposal,
            signature,
            _pd: PhantomData,
        });
        leaf = leaf_new_view;
        parent_state = state_new_view;
    }

    (proposals, votes)
}

/// create a quorum proposal
pub async fn build_quorum_proposal(
    handle: &SystemContextHandle<TestTypes, MemoryImpl>,
    upgrade_certificate: Option<UpgradeCertificate<TestTypes>>,
    private_key: &<BLSPubKey as SignatureKey>::PrivateKey,
    view: u64,
) -> Proposal<TestTypes, QuorumProposal<TestTypes>> {
    let public_key = &BLSPubKey::from_private(private_key);
    let (proposal, signature) = build_quorum_proposal_and_signature(
        handle,
        upgrade_certificate,
        private_key,
        public_key,
        view,
    )
    .await;
    Proposal {
        data: proposal,
        signature,
        _pd: PhantomData,
    }
}

/// get the keypair for a node id
#[must_use]
pub fn key_pair_for_id(node_id: u64) -> (<BLSPubKey as SignatureKey>::PrivateKey, BLSPubKey) {
    let private_key =
        <BLSPubKey as SignatureKey>::generated_from_seed_indexed([0u8; 32], node_id).1;
    let public_key = <TestTypes as NodeType>::SignatureKey::from_private(&private_key);
    (private_key, public_key)
}

/// initialize VID
/// # Panics
/// if unable to create a [`VidScheme`]
#[must_use]
pub fn vid_init<TYPES: NodeType>(
    membership: &TYPES::Membership,
    view_number: TYPES::Time,
) -> VidScheme {
    let num_committee = membership.get_committee(view_number).len();

    // calculate the last power of two
    // TODO change after https://github.com/EspressoSystems/jellyfish/issues/339
    // issue: https://github.com/EspressoSystems/HotShot/issues/2152
    let chunk_size = 1 << num_committee.ilog2();

    // TODO <https://github.com/EspressoSystems/HotShot/issues/1686>
    let srs = hotshot_types::data::test_srs(num_committee);

    VidScheme::new(chunk_size, num_committee, srs).unwrap()
}

pub fn build_vid_proposal(
    quorum_membership: &<TestTypes as NodeType>::Membership,
    view_number: ViewNumber,
    private_key: &<BLSPubKey as SignatureKey>::PrivateKey,
) -> Proposal<TestTypes, VidDisperse<TestTypes>> {
    let vid = vid_init::<TestTypes>(&quorum_membership, view_number);
    let transactions = vec![];
    let encoded_transactions = TestTransaction::encode(transactions.clone()).unwrap();
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();
    let payload_commitment = vid_disperse.commit;

    let vid_signature =
        <TestTypes as NodeType>::SignatureKey::sign(&private_key, payload_commitment.as_ref())
            .expect("Failed to sign payload commitment");
    let vid_disperse = VidDisperse::from_membership(
        view_number,
        vid.disperse(&encoded_transactions).unwrap(),
        &quorum_membership.clone().into(),
    );

    Proposal {
        data: vid_disperse.clone(),
        signature: vid_signature,
        _pd: PhantomData,
    }
}

pub fn build_da_certificate(
    quorum_membership: &<TestTypes as NodeType>::Membership,
    view_number: ViewNumber,
    public_key: &<TestTypes as NodeType>::SignatureKey,
    private_key: &<BLSPubKey as SignatureKey>::PrivateKey,
) -> DACertificate<TestTypes> {
    let block = <TestBlockPayload as TestableBlock>::genesis();

    let da_payload_commitment = vid_commitment(&Vec::new(), quorum_membership.total_nodes());

    let da_data = DAData {
        payload_commit: da_payload_commitment,
    };

    build_cert::<TestTypes, DAData, DAVote<TestTypes>, DACertificate<TestTypes>>(
        da_data,
        &quorum_membership,
        view_number,
        &public_key,
        &private_key,
    )
}

pub async fn build_vote(
    handle: &SystemContextHandle<TestTypes, MemoryImpl>,
    proposal: QuorumProposal<TestTypes>,
) -> GeneralConsensusMessage<TestTypes> {
    let consensus_lock = handle.get_consensus();
    let consensus = consensus_lock.read().await;
    let membership = handle.hotshot.memberships.quorum_membership.clone();

    let justify_qc = proposal.justify_qc.clone();
    let view = ViewNumber::new(*proposal.view_number);
    let parent = if justify_qc.is_genesis {
        let Some(genesis_view) = consensus.validated_state_map.get(&ViewNumber::new(0)) else {
            panic!("Couldn't find genesis view in state map.");
        };
        let Some(leaf) = genesis_view.get_leaf_commitment() else {
            panic!("Genesis view points to a view without a leaf");
        };
        let Some(leaf) = consensus.saved_leaves.get(&leaf) else {
            panic!("Failed to find genesis leaf.");
        };
        leaf.clone()
    } else {
        consensus
            .saved_leaves
            .get(&justify_qc.get_data().leaf_commit)
            .cloned()
            .unwrap()
    };

    let parent_commitment = parent.commit();

    let leaf: Leaf<_> = Leaf {
        view_number: view,
        justify_qc: proposal.justify_qc.clone(),
        parent_commitment,
        block_header: proposal.block_header,
        block_payload: None,
        proposer_id: membership.get_leader(view),
    };
    let vote = QuorumVote::<TestTypes>::create_signed_vote(
        QuorumData {
            leaf_commit: leaf.commit(),
        },
        view,
        handle.public_key(),
        handle.private_key(),
    )
    .expect("Failed to create quorum vote");
    GeneralConsensusMessage::<TestTypes>::Vote(vote)
}
