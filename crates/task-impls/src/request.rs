// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use async_broadcast::{Receiver, Sender};
use async_compatibility_layer::art::async_timeout;
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
        node_implementation::{NodeImplementation, NodeType},
        signature_key::SignatureKey,
    },
    vote::HasViewNumber,
};
use sha2::{Digest, Sha256};
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
                if prop_view >= self.view {
                    self.spawn_requests(prop_view, sender, receiver).await;
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
            // HotShotEvent::VidResponseRecv(proposal) => {
            //     // Make sure that this request came from who we think it did
            //     let state = &mut self.state.read().await;
            //     if let Some(Some(vid_share)) = state
            //         .vid_shares()
            //         .get(&proposal.data.view_number())
            //         .map(|shares| shares.get(&self.public_key).cloned())
            //     {
            //         broadcast_event(
            //             Arc::new(HotShotEvent::VidShareRecv(vid_share.clone())),
            //             &sender,
            //         )
            //         .await;
            //     }
            //     Ok(())
            // }
            _ => Ok(()),
        }
    }

    async fn cancel_subtasks(&mut self) {}
}

impl<TYPES: NodeType, I: NodeImplementation<TYPES>> NetworkRequestState<TYPES, I> {
    /// Spawns tasks for a given view to retrieve any data needed.
    async fn spawn_requests(
        &mut self,
        view: TYPES::Time,
        sender: &Sender<Arc<HotShotEvent<TYPES>>>,
        receiver: &Receiver<Arc<HotShotEvent<TYPES>>>,
    ) {
        // If we already have the VID shares for the next view, do nothing.
        let state = &mut self.state.read().await;
        if state.vid_shares().contains_key(&view) {
            return;
        }

        let request = ProposalRequestPayload {
            view_number: view,
            key: self.public_key.clone(),
        };

        // First sign the request for the VID shares.
        if let Some(signature) = self.serialize_and_sign(&request) {
            broadcast_event(
                HotShotEvent::VidRequestSend(request, signature).into(),
                sender,
            )
            .await;

            let Ok(Some(response)) = async_timeout(REQUEST_TIMEOUT*2, async move {
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
            .await
            else {
                // tracing::error!("no response");
                return;
            };

            // tracing::error!("broadcast: {:?}", response);
            broadcast_event(
                Arc::new(HotShotEvent::VidShareRecv(response.clone())),
                sender,
            )
            .await;
        }
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
