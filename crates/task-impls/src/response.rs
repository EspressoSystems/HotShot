// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{sync::Arc, time::Duration};

use async_broadcast::{Receiver, Sender};
use async_compatibility_layer::art::{async_sleep, async_spawn};
#[cfg(async_executor_impl = "async-std")]
use async_std::task::JoinHandle;
use hotshot_types::{
    consensus::{Consensus, LockedConsensusState, OuterConsensus},
    data::VidDisperseShare,
    message::Proposal,
    request_response::ProposalRequestPayload,
    traits::{election::Membership, node_implementation::NodeType, signature_key::SignatureKey},
};
use sha2::{Digest, Sha256};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::JoinHandle;
use tracing::instrument;

use crate::{events::HotShotEvent, helpers::broadcast_event};
/// Time to wait for txns before sending `ResponseMessage::NotFound`
const TXNS_TIMEOUT: Duration = Duration::from_millis(100);

/// Task state for the Network Request Task. The task is responsible for handling
/// requests sent to this node by the network.  It will validate the sender,
/// parse the request, and try to find the data request in the consensus stores.
pub struct NetworkResponseState<TYPES: NodeType> {
    /// Locked consensus state
    consensus: LockedConsensusState<TYPES>,
    /// Quorum membership for checking if requesters have state
    quorum: Arc<TYPES::Membership>,
    /// This replicas public key
    pub_key: TYPES::SignatureKey,
    /// This replicas private key
    private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
    /// The node's id
    id: u64,
}

impl<TYPES: NodeType> NetworkResponseState<TYPES> {
    /// Create the network request state with the info it needs
    pub fn new(
        consensus: LockedConsensusState<TYPES>,
        quorum: Arc<TYPES::Membership>,
        pub_key: TYPES::SignatureKey,
        private_key: <TYPES::SignatureKey as SignatureKey>::PrivateKey,
        id: u64,
    ) -> Self {
        Self {
            consensus,
            quorum,
            pub_key,
            private_key,
            id,
        }
    }

    /// Run the request response loop until a `HotShotEvent::Shutdown` is received.
    /// Or the stream is closed.
    async fn run_loop(
        self,
        mut receiver: Receiver<Arc<HotShotEvent<TYPES>>>,
        sender: Sender<Arc<HotShotEvent<TYPES>>>,
    ) {
        loop {
            match receiver.recv_direct().await {
                Ok(event) => {
                    // break loop when false, this means shutdown received
                    if !self.handle_event(event, &sender).await {
                        return;
                    }
                }
                Err(_) => {
                    tracing::error!("error");
                }
            }
        }
    }

    /// Handle an incoming message.  First validates the sender, then handles the contained request.
    /// Sends the response via `chan`
    async fn handle_event(
        &self,
        event: Arc<HotShotEvent<TYPES>>,
        send_stream: &Sender<Arc<HotShotEvent<TYPES>>>,
    ) -> bool {
        match event.as_ref() {
            HotShotEvent::VidRequestRecv(request, sender) => {
                if !self.valid_sender(&request.key) || !valid_signature::<TYPES>(request, sender) {
                    tracing::warn!("Request not valid!");
                    return true;
                }
                if let Some(proposal) = self
                    .get_or_calc_vid_share(request.view_number, &self.pub_key.clone())
                    .await
                {
                    broadcast_event(
                        HotShotEvent::VidResponseSend(request.key.clone(), proposal).into(),
                        send_stream,
                    )
                    .await;
                }
            }
            HotShotEvent::Shutdown => {
                tracing::error!("received shutdown in runloop");
                return false;
            }
            _ => {}
        }

        true
    }

    /// Get the VID share from consensus storage, or calculate it from the payload for
    /// the view, if we have the payload.  Stores all the shares calculated from the payload
    /// if the calculation was done
    #[instrument(skip_all, target = "NetworkResponseState", fields(id = self.id))]
    async fn get_or_calc_vid_share(
        &self,
        view: TYPES::Time,
        key: &TYPES::SignatureKey,
    ) -> Option<Proposal<TYPES, VidDisperseShare<TYPES>>> {
        let contained = self
            .consensus
            .read()
            .await
            .vid_shares()
            .get(&view)
            .is_some_and(|m| m.contains_key(key));
        if !contained {
            if Consensus::calculate_and_update_vid(
                OuterConsensus::new(Arc::clone(&self.consensus)),
                view,
                Arc::clone(&self.quorum),
                &self.private_key,
            )
            .await
            .is_none()
            {
                // Sleep in hope we receive txns in the meantime
                async_sleep(TXNS_TIMEOUT).await;
                Consensus::calculate_and_update_vid(
                    OuterConsensus::new(Arc::clone(&self.consensus)),
                    view,
                    Arc::clone(&self.quorum),
                    &self.private_key,
                )
                .await?;
            }
            return self
                .consensus
                .read()
                .await
                .vid_shares()
                .get(&view)?
                .get(key)
                .cloned();
        }
        self.consensus
            .read()
            .await
            .vid_shares()
            .get(&view)?
            .get(key)
            .cloned()
    }

    /// Makes sure the sender is allowed to send a request.
    fn valid_sender(&self, sender: &TYPES::SignatureKey) -> bool {
        self.quorum.has_stake(sender)
    }
}

/// Check the signature
fn valid_signature<TYPES: NodeType>(
    req: &ProposalRequestPayload<TYPES>,
    sender_sig: &<<TYPES as NodeType>::SignatureKey as SignatureKey>::PureAssembledSignatureType,
) -> bool {
    let Ok(data) = bincode::serialize(&req) else {
        tracing::error!("serialize failed");
        return false;
    };
    req.key.validate(sender_sig, &Sha256::digest(data))
}

/// Spawn the network response task to handle incoming request for data
/// from other nodes.  It will shutdown when it gets `HotshotEvent::Shutdown`
/// on the `event_stream` arg.
pub fn run_response_task<TYPES: NodeType>(
    task_state: NetworkResponseState<TYPES>,
    event_stream: Receiver<Arc<HotShotEvent<TYPES>>>,
    sender: Sender<Arc<HotShotEvent<TYPES>>>,
) -> JoinHandle<()> {
    async_spawn(task_state.run_loop(event_stream, sender))
}
