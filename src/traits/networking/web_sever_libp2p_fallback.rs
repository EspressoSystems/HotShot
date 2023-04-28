//! Networking Implementation that has a primary and a fallback newtork.  If the primary 
//! Errors we will use the backup to send or receive
use super::NetworkError;
use crate::traits::implementations::Libp2pNetwork;
use crate::traits::implementations::WebServerNetwork;
use crate::NodeImplementation;

use async_trait::async_trait;

use futures::join;

use hotshot_types::traits::network::TestableChannelImplementation;
use hotshot_types::traits::network::TestableNetworkingImplementation;
use hotshot_types::traits::network::ViewMessage;
use hotshot_types::{
    data::ProposalType,
    message::Message,
    traits::{
        election::Membership,
        network::{CommunicationChannel, ConnectedNetwork, TransmitType},
        node_implementation::NodeType,
        signature_key::TestableSignatureKey,
    },
    vote::VoteType,
};
use std::{marker::PhantomData, sync::Arc};

/// A communication channel with 2 networks, where we can fall back to the slower network if the
/// primary fails
#[derive(Clone)]
pub struct WebServerWithFallbackCommChannel<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
> {
    networks: Arc<CombinedNetworks<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>>,
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > WebServerWithFallbackCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
{
    #[must_use]
    pub fn new(networks: Arc<CombinedNetworks<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>>) -> Self {
        Self { networks }
    }

    pub fn network(
        &self,
    ) -> &WebServerNetwork<
        Message<TYPES, I>,
        TYPES::SignatureKey,
        TYPES::ElectionConfigType,
        TYPES,
        PROPOSAL,
        VOTE,
    > {
        &self.networks.0
    }
    pub fn fallback(&self) -> &Libp2pNetwork<Message<TYPES, I>, TYPES::SignatureKey> {
        &self.networks.1
    }
}
pub struct CombinedNetworks<
    TYPES: NodeType,
    I: NodeImplementation<TYPES>,
    PROPOSAL: ProposalType<NodeType = TYPES>,
    VOTE: VoteType<TYPES>,
    MEMBERSHIP: Membership<TYPES>,
>(
    WebServerNetwork<
        Message<TYPES, I>,
        TYPES::SignatureKey,
        TYPES::ElectionConfigType,
        TYPES,
        PROPOSAL,
        VOTE,
    >,
    Libp2pNetwork<Message<TYPES, I>, TYPES::SignatureKey>,
    PhantomData<MEMBERSHIP>,
);

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > TestableNetworkingImplementation<TYPES, Message<TYPES, I>>
    for CombinedNetworks<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    fn generator(
        expected_node_count: usize,
        num_bootstrap: usize,
        network_id: usize,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        let generators = (
            <WebServerNetwork<
                Message<TYPES, I>,
                TYPES::SignatureKey,
                TYPES::ElectionConfigType,
                TYPES,
                PROPOSAL,
                VOTE,
            > as TestableNetworkingImplementation<_, _>>::generator(
                expected_node_count,
                num_bootstrap,
                network_id,
            ),
            <Libp2pNetwork<Message<TYPES, I>, TYPES::SignatureKey> as TestableNetworkingImplementation<_, _>>::generator(expected_node_count, num_bootstrap, network_id)
        );
        Box::new(move |node_id| {
            CombinedNetworks(
                generators.0(node_id),
                generators.1(node_id),
                PhantomData::default(),
            )
        })
    }

    /// Get the number of messages in-flight.
    ///
    /// Some implementations will not be able to tell how many messages there are in-flight. These implementations should return `None`.
    fn in_flight_message_count(&self) -> Option<usize> {
        None
    }
}

#[async_trait]
impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    > CommunicationChannel<TYPES, Message<TYPES, I>, PROPOSAL, VOTE, MEMBERSHIP>
    for WebServerWithFallbackCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
{
    type NETWORK = CombinedNetworks<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>;

    async fn wait_for_ready(&self) {
        self.network().wait_for_ready().await;
        self.fallback().wait_for_ready().await
    }

    async fn is_ready(&self) -> bool {
        self.network().is_ready().await && self.fallback().is_ready().await
    }

    async fn shut_down(&self) -> () {
        self.network().shut_down().await;
        self.fallback().shut_down().await;
    }

    async fn broadcast_message(
        &self,
        message: Message<TYPES, I>,
        election: &MEMBERSHIP,
    ) -> Result<(), NetworkError> {
        let recipients =
            <MEMBERSHIP as Membership<TYPES>>::get_committee(election, message.get_view_number());
        let fallback = self
            .fallback()
            .broadcast_message(message.clone(), recipients.clone());
        let network = self.network().broadcast_message(message, recipients);
        match join!(fallback, network) {
            (Err(e), Err(_)) => Err(e),
            _ => Ok(()),
        }
    }

    async fn direct_message(
        &self,
        message: Message<TYPES, I>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        match self
            .network()
            .direct_message(message.clone(), recipient.clone())
            .await
        {
            Ok(_) => Ok(()),
            Err(_e) => {
                // TODO log e
                self.fallback().direct_message(message, recipient).await
            }
        }
    }

    async fn recv_msgs(
        &self,
        transmit_type: TransmitType,
    ) -> Result<Vec<Message<TYPES, I>>, NetworkError> {
        match self.network().recv_msgs(transmit_type.clone()).await {
            Ok(msgs) => Ok(msgs),
            Err(_e) => {
                // TODO log e
                self.fallback().recv_msgs(transmit_type).await
            }
        }
    }

    async fn lookup_node(&self, pk: TYPES::SignatureKey) -> Result<(), NetworkError> {
        match self.network().lookup_node(pk.clone()).await {
            Ok(msgs) => Ok(msgs),
            Err(_e) => {
                // TODO log e
                self.fallback().lookup_node(pk).await
            }
        }
    }

    async fn inject_consensus_info(&self, tuple: (u64, bool, bool)) -> Result<(), NetworkError> {
        <WebServerNetwork<_, _, _, _, _, _> as ConnectedNetwork<
            Message<TYPES, I>,
            TYPES::SignatureKey,
        >>::inject_consensus_info(self.network(), tuple)
        .await
    }
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
    >
    TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        PROPOSAL,
        VOTE,
        MEMBERSHIP,
        CombinedNetworks<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>,
    > for WebServerWithFallbackCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    fn generate_network() -> Box<dyn Fn(Arc<Self::NETWORK>) -> Self + 'static> {
        Box::new(move |network| WebServerWithFallbackCommChannel::new(network))
    }
}
