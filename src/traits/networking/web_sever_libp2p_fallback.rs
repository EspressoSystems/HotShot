use super::{
    memory_network::MemoryNetwork, FailedToSerializeSnafu, NetworkError, NetworkReliability,
    NetworkingMetrics,
};
use crate::NodeImplementation;
use async_compatibility_layer::{
    art::{async_sleep, async_spawn},
    channel::{bounded, Receiver, SendError, Sender},
};
use async_lock::{Mutex, RwLock};
use async_trait::async_trait;
use bincode::Options;
use dashmap::DashMap;
use futures::StreamExt;
use hotshot_types::traits::network::TestableChannelImplementation;
use hotshot_types::traits::network::ViewMessage;
use hotshot_types::{
    data::ProposalType,
    message::Message,
    traits::{
        election::Membership,
        metrics::{Metrics, NoMetrics},
        network::{
            CommunicationChannel, ConnectedNetwork, NetworkMsg, TestableNetworkingImplementation,
            TransmitType,
        },
        node_implementation::NodeType,
        signature_key::{SignatureKey, TestableSignatureKey},
    },
    vote::VoteType,
};
use hotshot_utils::bincode::bincode_opts;
use rand::Rng;
use snafu::ResultExt;
use std::{
    collections::BTreeSet,
    fmt::Debug,
    marker::PhantomData,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use crate::traits::implementations::WebServerNetwork;
use crate::traits::implementations::Libp2pNetwork;
use tracing::{debug, error, info, info_span, instrument, trace, warn, Instrument};

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
    networks: Arc<(
        WebServerNetwork<
            Message<TYPES, I>,
            TYPES::SignatureKey,
            TYPES::ElectionConfigType,
            TYPES,
            PROPOSAL,
            VOTE,
        >,
        Libp2pNetwork<Message<TYPES, I>, TYPES::SignatureKey>,
    )>,
    _pd: PhantomData<(I, PROPOSAL, VOTE, MEMBERSHIP)>,
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
    pub fn new(
        networks: Arc<(
            WebServerNetwork<
                Message<TYPES, I>,
                TYPES::SignatureKey,
                TYPES::ElectionConfigType,
                TYPES,
                PROPOSAL,
                VOTE,
            >,
            Libp2pNetwork<Message<TYPES, I>, TYPES::SignatureKey>,
        )>,
    ) -> Self {
        Self {
            networks,
            _pd: PhantomData::default(),
        }
    }

    pub fn network(
        &self,
    ) -> WebServerNetwork<
        Message<TYPES, I>,
        TYPES::SignatureKey,
        TYPES::ElectionConfigType,
        TYPES,
        PROPOSAL,
        VOTE,
    > {
        self.networks.0
    }
    pub fn fallback(&self) -> Libp2pNetwork<Message<TYPES, I>, TYPES::SignatureKey> {
        self.networks.1
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
    type NETWORK = (
        WebServerNetwork<
            Message<TYPES, I>,
            TYPES::SignatureKey,
            TYPES::ElectionConfigType,
            TYPES,
            PROPOSAL,
            VOTE,
        >,
        Libp2pNetwork<Message<TYPES, I>, TYPES::SignatureKey>,
    );

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
        /// TODO return no error if either succeeds
        self.fallback().broadcast_message(message, recipients).await;
        self.network().broadcast_message(message, recipients).await
    }

    async fn direct_message(
        &self,
        message: Message<TYPES, I>,
        recipient: TYPES::SignatureKey,
    ) -> Result<(), NetworkError> {
        match self.network().direct_message(message, recipient).await {
            Ok(_) => Ok(()),
            Err(e) => {
                // TODO log e
                self.fallback().direct_message(message, recipient).await
            }
        }
    }

    async fn recv_msgs(
        &self,
        transmit_type: TransmitType,
    ) -> Result<Vec<Message<TYPES, I>>, NetworkError> {
        match self.network().recv_msgs(transmit_type).await {
            Ok(msgs) => Ok(msgs),
            Err(e) => {
                // TODO log e
                self.fallback().recv_msgs(transmit_type).await
            }
        }
    }

    async fn lookup_node(&self, pk: TYPES::SignatureKey) -> Result<(), NetworkError> {
        match self.network().lookup_node(pk).await {
            Ok(msgs) => Ok(msgs),
            Err(e) => {
                // TODO log e
                self.fallback().lookup_node(pk).await
            }
        }
    }

    async fn inject_consensus_info(&self, _tuple: (u64, bool, bool)) -> Result<(), NetworkError> {
        // Not required
        Ok(())
    }
}

impl<
        TYPES: NodeType,
        I: NodeImplementation<TYPES>,
        PROPOSAL: ProposalType<NodeType = TYPES>,
        VOTE: VoteType<TYPES>,
        MEMBERSHIP: Membership<TYPES>,
        PRIMARY: ConnectedNetwork<Message<TYPES, I>, TYPES::SignatureKey>,
        FALLBACK: ConnectedNetwork<Message<TYPES, I>, TYPES::SignatureKey>,
    >
    TestableChannelImplementation<
        TYPES,
        Message<TYPES, I>,
        PROPOSAL,
        VOTE,
        MEMBERSHIP,
        (PRIMARY, FALLBACK),
    > for WebServerWithFallbackCommChannel<TYPES, I, PROPOSAL, VOTE, MEMBERSHIP>
where
    TYPES::SignatureKey: TestableSignatureKey,
{
    fn generate_network() -> Box<dyn Fn(Arc<Self::NETWORK>) -> Self + 'static> {
        Box::new(move |network| WebServerWithFallbackCommChannel::new(network))
    }
}

// #[cfg(test)]
// // panic in tests
// #[allow(clippy::panic)]
// mod tests {
//     use super::*;
//     use crate::{
//         demos::vdemo::{Addition, Subtraction, VDemoBlock, VDemoState, VDemoTransaction},
//         traits::{
//             election::static_committee::{
//                 GeneralStaticCommittee, StaticElectionConfig, StaticVoteToken,
//             },
//             networking::memory_network::MemoryNetwork,
//         },
//     };

//     use crate::traits::implementations::MasterMap;
//     use crate::traits::implementations::MemoryCommChannel;
//     use crate::traits::implementations::MemoryStorage;
//     use async_compatibility_layer::logging::setup_logging;
//     use hotshot_types::traits::election::QuorumExchange;
//     use hotshot_types::{
//         data::ViewNumber,
//         message::{DataMessage, MessageKind},
//         traits::{
//             signature_key::ed25519::{Ed25519Priv, Ed25519Pub},
//             state::ConsensusTime,
//         },
//         vote::QuorumVote,
//     };
//     use hotshot_types::{
//         data::{ValidatingLeaf, ValidatingProposal},
//         traits::state::ValidatingConsensus,
//     };
//     #[derive(
//         Copy,
//         Clone,
//         Debug,
//         Default,
//         Hash,
//         PartialEq,
//         Eq,
//         PartialOrd,
//         Ord,
//         serde::Serialize,
//         serde::Deserialize,
//     )]
//     struct Test {}
//     #[derive(Clone, Debug)]
//     struct TestImpl {}

//     // impl NetworkMsg for Test {}

//     impl NodeType for Test {
//         // TODO (da) can this be SequencingConsensus?
//         type ConsensusType = ValidatingConsensus;
//         type Time = ViewNumber;
//         type BlockType = VDemoBlock;
//         type SignatureKey = Ed25519Pub;
//         type VoteTokenType = StaticVoteToken<Ed25519Pub>;
//         type Transaction = VDemoTransaction;
//         type ElectionConfigType = StaticElectionConfig;
//         type StateType = VDemoState;
//     }

//     type TestMembership = GeneralStaticCommittee<Test, TestLeaf, Ed25519Pub>;
//     type TestNetwork =
//         WebServerWithFallbackCommChannel<Test, TestImpl, TestProposal, TestVote, TestMembership>;

//     impl NodeImplementation<Test> for TestImpl {
//         type QuorumExchange = QuorumExchange<
//             Test,
//             TestLeaf,
//             TestProposal,
//             TestMembership,
//             TestNetwork,
//             Message<Test, Self>,
//         >;
//         type CommitteeExchange = Self::QuorumExchange;
//         type Leaf = TestLeaf;
//         type Storage = MemoryStorage<Test, TestLeaf>;
//     }

//     type TestLeaf = ValidatingLeaf<Test>;
//     type TestVote = QuorumVote<Test, TestLeaf>;
//     type TestProposal = ValidatingProposal<Test, TestLeaf>;

//     /// fake Eq
//     /// we can't compare the votetokentype for equality, so we can't
//     /// derive EQ on `VoteType<TYPES>` and thereby message
//     /// we are only sending data messages, though so we compare key and
//     /// data message
//     fn fake_message_eq(message_1: Message<Test, TestImpl>, message_2: Message<Test, TestImpl>) {
//         assert_eq!(message_1.sender, message_2.sender);
//         if let MessageKind::Data(DataMessage::SubmitTransaction(d_1, _)) = message_1.kind {
//             if let MessageKind::Data(DataMessage::SubmitTransaction(d_2, _)) = message_2.kind {
//                 assert_eq!(d_1, d_2);
//             }
//         } else {
//             panic!("Got unexpected message type in memory test!");
//         }
//     }

//     #[instrument]
//     fn get_pubkey() -> Ed25519Pub {
//         let priv_key = Ed25519Priv::generate();
//         Ed25519Pub::from_private(&priv_key)
//     }

//     /// create a message
//     fn gen_messages(num_messages: u64, seed: u64, pk: Ed25519Pub) -> Vec<Message<Test, TestImpl>> {
//         let mut messages = Vec::new();
//         for i in 0..num_messages {
//             let message = Message {
//                 sender: pk,
//                 kind: MessageKind::Data(DataMessage::SubmitTransaction(
//                     VDemoTransaction {
//                         add: Addition {
//                             account: "A".to_string(),
//                             amount: 50 + i + seed,
//                         },
//                         sub: Subtraction {
//                             account: "B".to_string(),
//                             amount: 50 + i + seed,
//                         },
//                         nonce: seed + i,
//                         padding: vec![50; 0],
//                     },
//                     <ViewNumber as ConsensusTime>::new(0),
//                 )),
//             };
//             messages.push(message);
//         }
//         messages
//     }
//     // Check to make sure direct queue works
//     #[cfg_attr(
//         feature = "tokio-executor",
//         tokio::test(flavor = "multi_thread", worker_threads = 2)
//     )]
//     #[cfg_attr(feature = "async-std-executor", async_std::test)]
//     #[allow(deprecated)]
//     #[instrument]
//     async fn direct_queue() {
//         setup_logging();
//         // Create some dummy messages

//         // Make and connect the networking instances
//         let group: Arc<MasterMap<Message<Test, TestImpl>, <Test as NodeType>::SignatureKey>> =
//             MasterMap::new();
//         let group2: Arc<MasterMap<Message<Test, TestImpl>, <Test as NodeType>::SignatureKey>> =
//             MasterMap::new();
//         trace!(?group);
//         let pub_key_1 = get_pubkey();
//         let network1 =
//             MemoryNetwork::new(pub_key_1, NoMetrics::boxed(), group.clone(), Option::None);
//         let pub_key_2 = get_pubkey();
//         let network2 = MemoryNetwork::new(pub_key_2, NoMetrics::boxed(), group, Option::None);
//         let fallback1 =
//             MemoryNetwork::new(pub_key_1, NoMetrics::boxed(), group2.clone(), Option::None);
//         let fallback2 = MemoryNetwork::new(pub_key_2, NoMetrics::boxed(), group2, Option::None);

//         let comm1 = CommunicationChannelWithFallback::new((network1, fallback1));
//         let comm2 = CommunicationChannelWithFallback::new((network2, fallback2));

//         let first_messages: Vec<Message<Test, TestImpl>> = gen_messages(5, 100, pub_key_1);

//         // Test 1 -> 2
//         // Send messages
//         for sent_message in first_messages {
//             comm1
//                 .direct_message(sent_message.clone(), pub_key_2)
//                 .await
//                 .expect("Failed to message node");
//             let mut recv_messages = comm2
//                 .recv_msgs(TransmitType::Direct)
//                 .await
//                 .expect("Failed to receive message");
//             let recv_message = recv_messages.pop().unwrap();
//             assert!(recv_messages.is_empty());
//             fake_message_eq(sent_message, recv_message);
//         }

//         let second_messages: Vec<Message<Test, TestImpl>> = gen_messages(5, 200, pub_key_2);

//         // Test 2 -> 1
//         // Send messages
//         for sent_message in second_messages {
//             comm2
//                 .direct_message(sent_message.clone(), pub_key_1)
//                 .await
//                 .expect("Failed to message node");
//             let mut recv_messages = comm1
//                 .recv_msgs(TransmitType::Direct)
//                 .await
//                 .expect("Failed to receive message");
//             let recv_message = recv_messages.pop().unwrap();
//             assert!(recv_messages.is_empty());
//             fake_message_eq(sent_message, recv_message);
//         }
//     }
// }
