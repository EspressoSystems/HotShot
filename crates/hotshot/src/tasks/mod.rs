//! Provides a number of tasks that run continuously

/// Provides trait to create task states from a `SystemContextHandle`
pub mod task_state;

use std::{sync::Arc, time::Duration};

use async_broadcast::{Receiver, Sender};
use async_compatibility_layer::art::{async_sleep, async_spawn};
use async_lock::RwLock;
use hotshot_task::task::{Task, TaskRegistry};
use hotshot_task_impls::{
    consensus::ConsensusTaskState,
    da::DATaskState,
    events::HotShotEvent,
    network::{NetworkEventTaskState, NetworkMessageTaskState},
    quorum_proposal::QuorumProposalTaskState,
    quorum_proposal_recv::QuorumProposalRecvTaskState,
    quorum_vote::QuorumVoteTaskState,
    request::NetworkRequestState,
    response::{run_response_task, NetworkResponseState, RequestReceiver},
    transactions::TransactionTaskState,
    upgrade::UpgradeTaskState,
    vid::VIDTaskState,
    view_sync::ViewSyncTaskState,
};
use hotshot_types::{
    constants::{Version01, VERSION_0_1},
    message::{Message, Messages},
    traits::{
        election::Membership,
        network::{ConnectedNetwork, ConsensusIntentEvent},
        node_implementation::{ConsensusTime, NodeImplementation, NodeType},
        storage::Storage,
    },
};
use tracing::error;

use crate::{tasks::task_state::CreateTaskState, types::SystemContextHandle, ConsensusApi};

/// event for global event stream
#[derive(Clone, Debug)]
pub enum GlobalEvent {
    /// shut everything down
    Shutdown,
    /// dummy (TODO delete later)
    Dummy,
}

/// Add tasks for network requests and responses
pub async fn add_request_network_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    let state = NetworkRequestState::<TYPES, I, Version01>::create_from(handle).await;

    let task = Task::new(tx, rx, Arc::clone(&task_reg), state);
    task_reg.run_task(task).await;
}

/// Add a task which responds to requests on the network.
pub async fn add_response_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    hs_rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    rx: RequestReceiver<TYPES>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    let state = NetworkResponseState::<TYPES>::new(
        handle.hotshot.get_consensus(),
        rx,
        handle.hotshot.memberships.quorum_membership.clone().into(),
        handle.public_key().clone(),
        handle.private_key().clone(),
    );
    task_reg
        .register(run_response_task::<TYPES, Version01>(state, hs_rx))
        .await;
}
/// Add the network task to handle messages and publish events.
pub async fn add_network_message_task<
    TYPES: NodeType,
    NET: ConnectedNetwork<Message<TYPES>, TYPES::SignatureKey>,
>(
    task_reg: Arc<TaskRegistry>,
    event_stream: Sender<Arc<HotShotEvent<TYPES>>>,
    channel: Arc<NET>,
) {
    let net = Arc::clone(&channel);
    let network_state: NetworkMessageTaskState<_> = NetworkMessageTaskState {
        event_stream: event_stream.clone(),
    };

    let network = Arc::clone(&net);
    let mut state = network_state.clone();
    let handle = async_spawn(async move {
        loop {
            let msgs = match network.recv_msgs().await {
                Ok(msgs) => Messages(msgs),
                Err(err) => {
                    error!("failed to receive messages: {err}");

                    // return zero messages so we sleep and try again
                    Messages(vec![])
                }
            };
            if msgs.0.is_empty() {
                // TODO: Stop sleeping here: https://github.com/EspressoSystems/HotShot/issues/2558
                async_sleep(Duration::from_millis(100)).await;
            } else {
                state.handle_messages(msgs.0).await;
            }
        }
    });
    task_reg.register(handle).await;
}
/// Add the network task to handle events and send messages.
pub async fn add_network_event_task<
    TYPES: NodeType,
    NET: ConnectedNetwork<Message<TYPES>, TYPES::SignatureKey>,
    S: Storage<TYPES> + 'static,
>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    channel: Arc<NET>,
    membership: TYPES::Membership,
    filter: fn(&Arc<HotShotEvent<TYPES>>) -> bool,
    storage: Arc<RwLock<S>>,
) {
    let network_state: NetworkEventTaskState<_, _, _> = NetworkEventTaskState {
        channel,
        view: TYPES::Time::genesis(),
        version: VERSION_0_1,
        membership,
        filter,
        storage,
    };
    let task = Task::new(tx, rx, Arc::clone(&task_reg), network_state);
    task_reg.run_task(task).await;
}

/// Setup polls for the given `consensus_state`
pub async fn inject_consensus_polls<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    consensus_state: &ConsensusTaskState<TYPES, I>,
) {
    // Poll (forever) for the latest quorum proposal
    consensus_state
        .quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForLatestProposal)
        .await;

    // Poll (forever) for upgrade proposals
    consensus_state
        .quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForUpgradeProposal(0))
        .await;

    // Poll (forever) for upgrade votes
    consensus_state
        .quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForUpgradeVotes(0))
        .await;

    // See if we're in the DA committee
    // This will not work for epochs (because dynamic subscription
    // With the Push CDN, we are _always_ polling for latest anyway.
    let is_da = consensus_state
        .committee_membership
        .get_whole_committee(<TYPES as NodeType>::Time::new(0))
        .contains(&consensus_state.public_key);

    // If we are, poll for latest DA proposal.
    if is_da {
        consensus_state
            .committee_network
            .inject_consensus_info(ConsensusIntentEvent::PollForLatestProposal)
            .await;
    }

    // Poll (forever) for the latest view sync certificate
    consensus_state
        .quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForLatestViewSyncCertificate)
        .await;
    // Start polling for proposals for the first view
    consensus_state
        .quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForProposal(1))
        .await;

    consensus_state
        .quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForDAC(1))
        .await;

    if consensus_state
        .quorum_membership
        .get_leader(TYPES::Time::new(1))
        == consensus_state.public_key
    {
        consensus_state
            .quorum_network
            .inject_consensus_info(ConsensusIntentEvent::PollForVotes(0))
            .await;
    }
}

/// Setup polls for the given `quorum_proposal`.
pub async fn inject_quorum_proposal_polls<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    quorum_proposal_task_state: &QuorumProposalTaskState<TYPES, I>,
) {
    // Poll (forever) for the latest view sync certificate
    quorum_proposal_task_state
        .quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForLatestViewSyncCertificate)
        .await;

    quorum_proposal_task_state
        .quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForDAC(1))
        .await;
}

/// Setup polls for the [`QuorumProposalRecvTaskState`].
pub async fn inject_quorum_proposal_recv_polls<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    quorum_proposal_recv_task_state: &QuorumProposalRecvTaskState<TYPES, I>,
) {
    // Poll (forever) for the latest quorum proposal
    quorum_proposal_recv_task_state
        .quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForLatestProposal)
        .await;

    // Start polling for proposals for the first view
    quorum_proposal_recv_task_state
        .quorum_network
        .inject_consensus_info(ConsensusIntentEvent::PollForProposal(1))
        .await;
}

/// add the consensus task
pub async fn add_consensus_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    let consensus_state = ConsensusTaskState::create_from(handle).await;

    inject_consensus_polls(&consensus_state).await;

    let task = Task::new(tx, rx, Arc::clone(&task_reg), consensus_state);
    task_reg.run_task(task).await;
}

/// add the VID task
pub async fn add_vid_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    let vid_state = VIDTaskState::create_from(handle).await;
    let task = Task::new(tx, rx, Arc::clone(&task_reg), vid_state);
    task_reg.run_task(task).await;
}

/// add the Upgrade task.
pub async fn add_upgrade_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    let upgrade_state = UpgradeTaskState::create_from(handle).await;

    let task = Task::new(tx, rx, Arc::clone(&task_reg), upgrade_state);
    task_reg.run_task(task).await;
}
/// add the Data Availability task
pub async fn add_da_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    // build the da task
    let da_state = DATaskState::create_from(handle).await;

    let task = Task::new(tx, rx, Arc::clone(&task_reg), da_state);
    task_reg.run_task(task).await;
}

/// add the Transaction Handling task
pub async fn add_transaction_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    let transactions_state = TransactionTaskState::<_, _, _, Version01>::create_from(handle).await;

    let task = Task::new(tx, rx, Arc::clone(&task_reg), transactions_state);
    task_reg.run_task(task).await;
}

/// add the view sync task
pub async fn add_view_sync_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    let view_sync_state = ViewSyncTaskState::create_from(handle).await;

    let task = Task::new(tx, rx, Arc::clone(&task_reg), view_sync_state);
    task_reg.run_task(task).await;
}

/// add the quorum proposal task
pub async fn add_quorum_proposal_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    let quorum_proposal_task_state = QuorumProposalTaskState::create_from(handle).await;
    let task = Task::new(tx, rx, Arc::clone(&task_reg), quorum_proposal_task_state);
    task_reg.run_task(task).await;
}

/// Add the quorum vote task.
pub async fn add_quorum_vote_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    let quorum_vote_state = QuorumVoteTaskState::create_from(handle).await;
    let task = Task::new(tx, rx, Arc::clone(&task_reg), quorum_vote_state);
    task_reg.run_task(task).await;
}

/// Add the quorum proposal recv task.
pub async fn add_quorum_proposal_recv_task<TYPES: NodeType, I: NodeImplementation<TYPES>>(
    task_reg: Arc<TaskRegistry>,
    tx: Sender<Arc<HotShotEvent<TYPES>>>,
    rx: Receiver<Arc<HotShotEvent<TYPES>>>,
    handle: &SystemContextHandle<TYPES, I>,
) {
    let quorum_proposal_recv_task_state = QuorumProposalTaskState::create_from(handle).await;
    inject_quorum_proposal_recv_polls(&quorum_proposal_recv_task_state).await;
    let task = Task::new(
        tx,
        rx,
        Arc::clone(&task_reg),
        quorum_proposal_recv_task_state,
    );
    task_reg.run_task(task).await;
}
