// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use anyhow::Result;
use async_broadcast::{Receiver, Sender};
use async_compatibility_layer::art::{async_sleep, async_spawn, async_timeout};
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use async_trait::async_trait;
use committable::Committable;
use hotshot_task::{
    dependency::{Dependency, EventDependency},
    task::TaskState,
};
use hotshot_types::{
    consensus::OuterConsensus,
    request_response::ProposalRequestPayload,
    traits::{
        network::ConnectedNetwork,
        node_implementation::{NodeImplementation, NodeType},
        signature_key::SignatureKey,
    },
    vote::HasViewNumber,
};
use sha2::{Digest, Sha256};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::instrument;

use crate::{events::HotShotEvent, helpers::broadcast_event};

/// Amount of time to try for a request before timing out.
pub const REQUEST_TIMEOUT: Duration = Duration::from_millis(500);

/// Long running task which will request information after a proposal is received.
/// The task will wait a it's `delay` and then send a request iteratively to peers
/// for any data they don't have related to the proposal.  For now it's just requesting VID
/// shares.
pub struct NetworkRequestState<TYPES: NodeType, I: NodeImplementation<TYPES>> {
    /// Network to send requests over
    /// The underlying network
    pub network: Arc<I::Network>,
    /// Consensus shared state so we can check if we've gotten the information
    /// before sending a request
    pub state: OuterConsensus<TYPES>,
    /// Last seen view, we won't request for proposals before older than this view
    pub view: TYPES::Time,
    /// Delay before requesting peers
    pub delay: Duration,
    /// DA Membership
    pub da_membership: TYPES::Membership,
    /// Quorum Membership
    pub quorum_membership: TYPES::Membership,
    /// This nodes public key
    pub public_key: TYPES::SignatureKey,
    /// This nodes private/signing key, used to sign requests.
    pub private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    /// The node's id
    pub id: u64,
    /// A flag indicating that `HotShotEvent::Shutdown` has been received
    pub spawned_tasks: BTreeMap<TYPES::Time, Vec<JoinHandle<()>>>,
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> Drop for NetworkRequestState<TYPES, I> {
    fn drop(&mut self) {
        futures::executor::block_on(async move { self.cancel_subtasks().await });
    }
}

/// Alias for a signature
type Signature<TYPES> =
    <<TYPES as NodeType>::SignatureKey as SignatureKey>::PureAssembledSignatureType;

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES>> TaskState for NetworkRequestState<TYPES, I> {
    type Event = HotShotEvent<TYPES>;

    #[instrument(skip_all, target = "NetworkRequestState", fields(id = self.id))]
    async fn handle_event(
        &mut self,
        event: Arc<Self::Event>,
        sender: &Sender<Arc<Self::Event>>,
        receiver: &Receiver<Arc<Self::Event>>,
    ) -> Result<()> {
        match event.as_ref() {
            HotShotEvent::QuorumProposalValidated(proposal, _) => {
                let prop_view = proposal.view_number();

                // If we already have the VID shares for the next view, do nothing.
                if prop_view >= self.view
                    && !self
                        .state
                        .read()
                        .await
                        .vid_shares()
                        .contains_key(&prop_view)
                {
                    self.spawn_requests(prop_view, sender, receiver);
                }
                Ok(())
            }
            HotShotEvent::ViewChange(view) => {
                let view = *view;
                if view > self.view {
                    self.view = view;
                }
                Ok(())
            }
            HotShotEvent::QuorumProposalRequestRecv(req, signature) => {
                // Make sure that this request came from who we think it did
                ensure!(
                    req.key.validate(signature, req.commit().as_ref()),
                    "Invalid signature key on proposal request."
                );

                if let Some(quorum_proposal) = self
                    .state
                    .read()
                    .await
                    .last_proposals()
                    .get(&req.view_number)
                {
                    broadcast_event(
                        HotShotEvent::QuorumProposalResponseSend(
                            req.key.clone(),
                            quorum_proposal.clone(),
                        )
                        .into(),
                        sender,
                    )
                    .await;
                }

                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn cancel_subtasks(&mut self) {
        while !self.spawned_tasks.is_empty() {
            let Some((_, handles)) = self.spawned_tasks.pop_first() else {
                break;
            };

            for handle in handles {
                #[cfg(async_executor_impl = "async-std")]
                handle.cancel().await;
                #[cfg(async_executor_impl = "tokio")]
                handle.abort();
            }
        }
    }
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> NetworkRequestState<TYPES, I> {
    /// Creates and signs the payload, then will create a request task
    fn spawn_requests(
        &mut self,
        view: TYPES::Time,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
        receiver: &Receiver<Arc<HotShotEvent<TYPES>>>,
    ) {
        let request = ProposalRequestPayload {
            view_number: view,
            key: self.public_key.clone(),
        };

        // First sign the request for the VID shares.
        if let Some(signature) = self.serialize_and_sign(&request) {
            self.create_vid_request_task(
                request,
                signature,
                sender.clone(),
                receiver.clone(),
                view,
            );
        }
    }

    /// Creates a task that will wait for the vid the response
    fn create_vid_request_task(
        &mut self,
        request: ProposalRequestPayload<TYPES>,
        signature: Signature<TYPES>,
        sender: Sender<Arc<HotShotEvent<TYPES>>>,
        receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        view: TYPES::Time,
    ) {
        let network = Arc::clone(&self.network);
        let delay = self.delay;
        async_spawn(async move {
            // Do the delay only if primary is up and then start sending
            if !network.is_primary_down() {
                async_sleep(delay).await;
            }
            broadcast_event(
                HotShotEvent::VidRequestSend(request, signature).into(),
                &sender,
            )
            .await;

            // start waiting
            for _ in 0..4 {
                let timeout = async_timeout(REQUEST_TIMEOUT, async {
                    let mut response = None;
                    while response.is_none() {
                        let event = EventDependency::new(
                            receiver.clone(),
                            Box::new(move |event: &Arc<HotShotEvent<TYPES>>| {
                                let event = event.as_ref();
                                if let HotShotEvent::VidResponseRecv(proposal) = event {
                                    proposal.data.view_number() == view
                                } else {
                                    false
                                }
                            }),
                        )
                        .completed()
                        .await;

                        if let Some(hs_event) = event.as_ref() {
                            if let HotShotEvent::VidResponseRecv(proposal) = hs_event.as_ref() {
                                response = Some(proposal.clone());
                            }
                        }
                    }

                    response
                })
                .await;

                // check if success otherwise retry until max attempts is reached
                if let Ok(Some(response)) = timeout {
                    broadcast_event(Arc::new(HotShotEvent::VidShareRecv(response)), &sender).await;
                    return;
                }
            }
        });
        // self.spawned_tasks.entry(view).or_default().push(handle);
    }

    /// Sign the serialized version of the request
    fn serialize_and_sign(
        &self,
        request: &ProposalRequestPayload<TYPES>,
    ) -> Option<Signature<TYPES>> {
        let Ok(data) = bincode::serialize(&request) else {
            tracing::error!("Failed to serialize request!");
            return None;
        };
        let Ok(signature) = TYPES::SignatureKey::sign(&self.private_key, &Sha256::digest(data))
        else {
            tracing::error!("Failed to sign Data Request");
            return None;
        };
        Some(signature)
    }
}
