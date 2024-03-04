use std::{sync::Arc, time::Duration};

use async_broadcast::Sender;
use async_lock::RwLock;
use hotshot_task::task::TaskState;
use hotshot_types::{
    consensus::Consensus,
    event,
    traits::{
        election::Membership, network::RequestKind, node_implementation::{NodeImplementation, NodeType}, signature_key::SignatureKey
    },
    vote::HasViewNumber,
};

use crate::events::{HotShotEvent, HotShotTaskCompleted};

pub struct NetworkResponseState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    pub network: I::QuorumNetwork,
    pub state: Arc<RwLock<Consensus<TYPES>>>,
    pub view: TYPES::Time,
    pub delay: Duration,
    pub da_membership: TYPES::Membership,
    pub quorum_membership: TYPES::Membership,
    pub public_key: TYPES::SignatureKey,
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TaskState for NetworkResponseState<TYPES, I> {
    type Event = HotShotEvent<TYPES>;

    type Output = HotShotTaskCompleted;

    async fn handle_event(
        event: Self::Event,
        task: &mut hotshot_task::task::Task<Self>,
    ) -> Option<Self::Output> {
        match event {
            HotShotEvent::QuorumProposalValidated(proposal) => {
                let state = task.state();
                let prop_view = proposal.get_view_number();
                if prop_view >= state.view {
                    state.spawn_requests(prop_view, task.clone_sender());
                }
                None
            }
            HotShotEvent::ViewChange(view) => {
                if view > task.state().view {
                    task.state_mut().view = view
                }
                None
            }
            HotShotEvent::Shutdown => Some(HotShotTaskCompleted),
            _ => None,
        }
    }

    fn should_shutdown(event: &Self::Event) -> bool {
        matches!(event, HotShotEvent::Shutdown)
    }
    fn filter(&self, event: &Self::Event) -> bool {
        !matches!(
            event,
            HotShotEvent::Shutdown
                | HotShotEvent::QuorumProposalValidated(_)
                | HotShotEvent::ViewChange(_)
        )
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> NetworkResponseState<TYPES, I> {
    fn spawn_requests(&self, view: TYPES::Time, sender: Sender<HotShotEvent<TYPES>>) {
        /// Cloning Arcs
        let net = self.network.clone();
        let state = self.state.clone();

        let membership = self.da_membership.clone();
        let delay = self.delay;

        let requests = self.build_requests(view).await;

    }

    async fn build_requests(&self, view: TYPES::Time) -> Vec<RequestKind<TYPES>> {
        let reqs = Vec::new();
        if !self.state.read().await.(view) {

        }
        reqs
    }
    fn run_delay(request: RequestKind<TYPES>, delay: Duration) {
        // async_compatibility_layer::art::async_spawn()
    }
}

struct DelayedRequester<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    network: I::QuorumNetwork
    state: Arc<RwLock<Consensus<TYPES>>>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> DelayedRequester<TYPES, I> {
    async fn run(self) {

    }
}


