// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

#![allow(clippy::panic)]
use std::{fmt::Debug, hash::Hash, marker::PhantomData, sync::Arc};

use async_broadcast::{Receiver, Sender};
use bitvec::bitvec;
use committable::Committable;
use hotshot::{
    traits::{NodeImplementation, TestableNodeImplementation},
    types::{SignatureKey, SystemContextHandle},
    HotShotInitializer, SystemContext,
};
use hotshot_example_types::{
    auction_results_provider_types::TestAuctionResultsProvider,
    block_types::TestTransaction,
    node_types::TestTypes,
    state_types::{TestInstanceState, TestValidatedState},
    storage_types::TestStorage,
};
use hotshot_task_impls::events::HotShotEvent;
use hotshot_types::{
    consensus::ConsensusMetricsValue,
    data::{Leaf, Leaf2, QuorumProposal, VidDisperse, VidDisperseShare},
    message::{GeneralConsensusMessage, Proposal, UpgradeLock},
    simple_certificate::DaCertificate2,
    simple_vote::{DaData2, DaVote2, QuorumData, QuorumVote, SimpleVote, VersionedVoteData},
    traits::{
        block_contents::vid_commitment,
        consensus_api::ConsensusApi,
        election::Membership,
        node_implementation::{NodeType, Versions},
    },
    utils::{View, ViewInner},
    vid::{vid_scheme, VidCommitment, VidProposal, VidSchemeType},
    vote::{Certificate, HasViewNumber, Vote},
    ValidatorConfig,
};
use jf_vid::VidScheme;
use primitive_types::U256;
use serde::Serialize;

use crate::{test_builder::TestDescription, test_launcher::TestLauncher};
/// create the [`SystemContextHandle`] from a node id
/// # Panics
/// if cannot create a [`HotShotInitializer`]
pub async fn build_system_handle<
    TYPES: NodeType<InstanceState = TestInstanceState>,
    I: NodeImplementation<
            TYPES,
            Storage = TestStorage<TYPES>,
            AuctionResultsProvider = TestAuctionResultsProvider<TYPES>,
        > + TestableNodeImplementation<TYPES>,
    V: Versions,
>(
    node_id: u64,
) -> (
    SystemContextHandle<TYPES, I, V>,
    Sender<Arc<HotShotEvent<TYPES>>>,
    Receiver<Arc<HotShotEvent<TYPES>>>,
) {
    let builder: TestDescription<TYPES, I, V> = TestDescription::default_multiple_rounds();

    let launcher = builder.gen_launcher(node_id);
    build_system_handle_from_launcher(node_id, &launcher).await
}

/// create the [`SystemContextHandle`] from a node id and `TestLauncher`
/// # Panics
/// if cannot create a [`HotShotInitializer`]
pub async fn build_system_handle_from_launcher<
    TYPES: NodeType<InstanceState = TestInstanceState>,
    I: NodeImplementation<
            TYPES,
            Storage = TestStorage<TYPES>,
            AuctionResultsProvider = TestAuctionResultsProvider<TYPES>,
        > + TestableNodeImplementation<TYPES>,
    V: Versions,
>(
    node_id: u64,
    launcher: &TestLauncher<TYPES, I, V>,
) -> (
    SystemContextHandle<TYPES, I, V>,
    Sender<Arc<HotShotEvent<TYPES>>>,
    Receiver<Arc<HotShotEvent<TYPES>>>,
) {
    let network = (launcher.resource_generator.channel_generator)(node_id).await;
    let storage = (launcher.resource_generator.storage)(node_id);
    let marketplace_config = (launcher.resource_generator.marketplace_config)(node_id);
    let config = launcher.resource_generator.config.clone();

    let initializer = HotShotInitializer::<TYPES>::from_genesis::<V>(TestInstanceState::new(
        launcher.metadata.async_delay_config.clone(),
    ))
    .await
    .unwrap();

    // See whether or not we should be DA
    let is_da = node_id < config.da_staked_committee_size as u64;

    // We assign node's public key and stake value rather than read from config file since it's a test
    let validator_config: ValidatorConfig<TYPES::SignatureKey> =
        ValidatorConfig::generated_from_seed_indexed([0u8; 32], node_id, 1, is_da);
    let private_key = validator_config.private_key.clone();
    let public_key = validator_config.public_key.clone();

    let memberships = TYPES::Membership::new(
        config.known_nodes_with_stake.clone(),
        config.known_da_nodes.clone(),
    );

    SystemContext::init(
        public_key,
        private_key,
        node_id,
        config,
        memberships,
        network,
        initializer,
        ConsensusMetricsValue::default(),
        storage,
        marketplace_config,
    )
    .await
    .expect("Could not init hotshot")
}

/// create certificate
/// # Panics
/// if we fail to sign the data
pub async fn build_cert<
    TYPES: NodeType,
    V: Versions,
    DATAType: Committable + Clone + Eq + Hash + Serialize + Debug + 'static,
    VOTE: Vote<TYPES, Commitment = DATAType>,
    CERT: Certificate<TYPES, VOTE::Commitment, Voteable = VOTE::Commitment>,
>(
    data: DATAType,
    da_membership: &TYPES::Membership,
    view: TYPES::View,
    epoch: TYPES::Epoch,
    public_key: &TYPES::SignatureKey,
    private_key: &<TYPES::SignatureKey as SignatureKey>::PrivateKey,
    upgrade_lock: &UpgradeLock<TYPES, V>,
) -> CERT {
    let real_qc_sig = build_assembled_sig::<TYPES, V, VOTE, CERT, DATAType>(
        &data,
        da_membership,
        view,
        epoch,
        upgrade_lock,
    )
    .await;

    let vote = SimpleVote::<TYPES, DATAType>::create_signed_vote(
        data,
        view,
        public_key,
        private_key,
        upgrade_lock,
    )
    .await
    .expect("Failed to sign data!");

    let vote_commitment =
        VersionedVoteData::new(vote.date().clone(), vote.view_number(), upgrade_lock)
            .await
            .expect("Failed to create VersionedVoteData!")
            .commit();

    let cert = CERT::create_signed_certificate(
        vote_commitment,
        vote.date().clone(),
        real_qc_sig,
        vote.view_number(),
    );
    cert
}

pub fn vid_share<TYPES: NodeType>(
    shares: &[Proposal<TYPES, VidDisperseShare<TYPES>>],
    pub_key: TYPES::SignatureKey,
) -> Proposal<TYPES, VidDisperseShare<TYPES>> {
    shares
        .iter()
        .filter(|s| s.data.recipient_key == pub_key)
        .cloned()
        .collect::<Vec<_>>()
        .first()
        .expect("No VID for key")
        .clone()
}

/// create signature
/// # Panics
/// if fails to convert node id into keypair
pub async fn build_assembled_sig<
    TYPES: NodeType,
    V: Versions,
    VOTE: Vote<TYPES>,
    CERT: Certificate<TYPES, VOTE::Commitment, Voteable = VOTE::Commitment>,
    DATAType: Committable + Clone + Eq + Hash + Serialize + Debug + 'static,
>(
    data: &DATAType,
    membership: &TYPES::Membership,
    view: TYPES::View,
    epoch: TYPES::Epoch,
    upgrade_lock: &UpgradeLock<TYPES, V>,
) -> <TYPES::SignatureKey as SignatureKey>::QcType {
    let stake_table = CERT::stake_table(membership, epoch);
    let real_qc_pp: <TYPES::SignatureKey as SignatureKey>::QcParams =
        <TYPES::SignatureKey as SignatureKey>::public_parameter(
            stake_table.clone(),
            U256::from(CERT::threshold(membership, epoch)),
        );
    let total_nodes = stake_table.len();
    let signers = bitvec![1; total_nodes];
    let mut sig_lists = Vec::new();

    // assemble the vote
    for node_id in 0..total_nodes {
        let (private_key_i, public_key_i) = key_pair_for_id::<TYPES>(node_id.try_into().unwrap());
        let vote: SimpleVote<TYPES, DATAType> = SimpleVote::<TYPES, DATAType>::create_signed_vote(
            data.clone(),
            view,
            &public_key_i,
            &private_key_i,
            upgrade_lock,
        )
        .await
        .expect("Failed to sign data!");
        let original_signature: <TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType =
            vote.signature();
        sig_lists.push(original_signature);
    }

    let real_qc_sig = <TYPES::SignatureKey as SignatureKey>::assemble(
        &real_qc_pp,
        signers.as_bitslice(),
        &sig_lists[..],
    );

    real_qc_sig
}

/// get the keypair for a node id
#[must_use]
pub fn key_pair_for_id<TYPES: NodeType>(
    node_id: u64,
) -> (
    <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    TYPES::SignatureKey,
) {
    let private_key = TYPES::SignatureKey::generated_from_seed_indexed([0u8; 32], node_id).1;
    let public_key = <TYPES as NodeType>::SignatureKey::from_private(&private_key);
    (private_key, public_key)
}

/// initialize VID
/// # Panics
/// if unable to create a [`VidSchemeType`]
#[must_use]
pub fn vid_scheme_from_view_number<TYPES: NodeType>(
    membership: &TYPES::Membership,
    view_number: TYPES::View,
    epoch_number: TYPES::Epoch,
) -> VidSchemeType {
    let num_storage_nodes = membership
        .committee_members(view_number, epoch_number)
        .len();
    vid_scheme(num_storage_nodes)
}

pub fn vid_payload_commitment<TYPES: NodeType>(
    quorum_membership: &<TYPES as NodeType>::Membership,
    view_number: TYPES::View,
    epoch_number: TYPES::Epoch,
    transactions: Vec<TestTransaction>,
) -> VidCommitment {
    let mut vid =
        vid_scheme_from_view_number::<TYPES>(quorum_membership, view_number, epoch_number);
    let encoded_transactions = TestTransaction::encode(&transactions);
    let vid_disperse = vid.disperse(&encoded_transactions).unwrap();

    vid_disperse.commit
}

pub fn da_payload_commitment<TYPES: NodeType>(
    quorum_membership: &<TYPES as NodeType>::Membership,
    transactions: Vec<TestTransaction>,
    epoch_number: TYPES::Epoch,
) -> VidCommitment {
    let encoded_transactions = TestTransaction::encode(&transactions);

    vid_commitment(
        &encoded_transactions,
        quorum_membership.total_nodes(epoch_number),
    )
}

pub fn build_payload_commitment<TYPES: NodeType>(
    membership: &<TYPES as NodeType>::Membership,
    view: TYPES::View,
    epoch: TYPES::Epoch,
) -> <VidSchemeType as VidScheme>::Commit {
    // Make some empty encoded transactions, we just care about having a commitment handy for the
    // later calls. We need the VID commitment to be able to propose later.
    let mut vid = vid_scheme_from_view_number::<TYPES>(membership, view, epoch);
    let encoded_transactions = Vec::new();
    vid.commit_only(&encoded_transactions).unwrap()
}

/// TODO: <https://github.com/EspressoSystems/HotShot/issues/2821>
pub fn build_vid_proposal<TYPES: NodeType>(
    quorum_membership: &<TYPES as NodeType>::Membership,
    view_number: TYPES::View,
    epoch_number: TYPES::Epoch,
    transactions: Vec<TestTransaction>,
    private_key: &<TYPES::SignatureKey as SignatureKey>::PrivateKey,
) -> VidProposal<TYPES> {
    let mut vid =
        vid_scheme_from_view_number::<TYPES>(quorum_membership, view_number, epoch_number);
    let encoded_transactions = TestTransaction::encode(&transactions);

    let vid_disperse = VidDisperse::from_membership(
        view_number,
        vid.disperse(&encoded_transactions).unwrap(),
        quorum_membership,
        epoch_number,
    );

    let signature =
        TYPES::SignatureKey::sign(private_key, vid_disperse.payload_commitment.as_ref())
            .expect("Failed to sign VID commitment");
    let vid_disperse_proposal = Proposal {
        data: vid_disperse.clone(),
        signature,
        _pd: PhantomData,
    };

    (
        vid_disperse_proposal,
        VidDisperseShare::from_vid_disperse(vid_disperse)
            .into_iter()
            .map(|vid_disperse| {
                vid_disperse
                    .to_proposal(private_key)
                    .expect("Failed to sign payload commitment")
            })
            .collect(),
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn build_da_certificate<TYPES: NodeType, V: Versions>(
    membership: &<TYPES as NodeType>::Membership,
    view_number: TYPES::View,
    epoch_number: TYPES::Epoch,
    transactions: Vec<TestTransaction>,
    public_key: &TYPES::SignatureKey,
    private_key: &<TYPES::SignatureKey as SignatureKey>::PrivateKey,
    upgrade_lock: &UpgradeLock<TYPES, V>,
) -> DaCertificate2<TYPES> {
    let encoded_transactions = TestTransaction::encode(&transactions);

    let da_payload_commitment =
        vid_commitment(&encoded_transactions, membership.total_nodes(epoch_number));

    let da_data = DaData2 {
        payload_commit: da_payload_commitment,
        epoch: epoch_number,
    };

    build_cert::<TYPES, V, DaData2<TYPES>, DaVote2<TYPES>, DaCertificate2<TYPES>>(
        da_data,
        membership,
        view_number,
        epoch_number,
        public_key,
        private_key,
        upgrade_lock,
    )
    .await
}

pub async fn build_vote<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions>(
    handle: &SystemContextHandle<TYPES, I, V>,
    proposal: QuorumProposal<TYPES>,
) -> GeneralConsensusMessage<TYPES> {
    let view = proposal.view_number;

    let leaf: Leaf<_> = Leaf::from_quorum_proposal(&proposal);
    let vote = QuorumVote::<TYPES>::create_signed_vote(
        QuorumData {
            leaf_commit: leaf.commit(&handle.hotshot.upgrade_lock).await,
        },
        view,
        &handle.public_key(),
        handle.private_key(),
        &handle.hotshot.upgrade_lock,
    )
    .await
    .expect("Failed to create quorum vote");
    GeneralConsensusMessage::<TYPES>::Vote(vote)
}

/// This function permutes the provided input vector `inputs`, given some order provided within the
/// `order` vector.
///
/// # Examples
/// let output = permute_input_with_index_order(vec![1, 2, 3], vec![2, 1, 0]);
/// // Output is [3, 2, 1] now
pub fn permute_input_with_index_order<T>(inputs: Vec<T>, order: Vec<usize>) -> Vec<T>
where
    T: Clone,
{
    let mut ordered_inputs = Vec::with_capacity(inputs.len());
    for &index in &order {
        ordered_inputs.push(inputs[index].clone());
    }
    ordered_inputs
}

/// This function will create a fake [`View`] from a provided [`Leaf`].
pub async fn build_fake_view_with_leaf<V: Versions>(
    leaf: Leaf2<TestTypes>,
    upgrade_lock: &UpgradeLock<TestTypes, V>,
) -> View<TestTypes> {
    build_fake_view_with_leaf_and_state(leaf, TestValidatedState::default(), upgrade_lock).await
}

/// This function will create a fake [`View`] from a provided [`Leaf`] and `state`.
pub async fn build_fake_view_with_leaf_and_state<V: Versions>(
    leaf: Leaf2<TestTypes>,
    state: TestValidatedState,
    _upgrade_lock: &UpgradeLock<TestTypes, V>,
) -> View<TestTypes> {
    View {
        view_inner: ViewInner::Leaf {
            leaf: leaf.commit(),
            state: state.into(),
            delta: None,
        },
    }
}
