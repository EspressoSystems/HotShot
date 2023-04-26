use hotshot::{
    traits::{
        election::static_committee::StaticCommittee,
        implementations::{Libp2pCommChannel, MemoryStorage},
    },
    types::Message,
};
use hotshot_testing::{
    test_description::GeneralTestDescriptionBuilder, test_types::StaticCommitteeTestTypes,
};
use hotshot_types::traits::{
    consensus_type::validating_consensus::ValidatingConsensus,
    election::QuorumExchange,
    node_implementation::{NodeImplementation, ValidatingExchanges},
};
use hotshot_types::{
    data::{ValidatingLeaf, ValidatingProposal},
    message::ValidatingMessage,
    vote::QuorumVote,
};
use serde::{Deserialize, Serialize};
use tracing::instrument;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Libp2pImpl {}

type StaticMembership =
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;

type StaticCommunication = Libp2pCommChannel<
    StaticCommitteeTestTypes,
    Libp2pImpl,
    ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    QuorumVote<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
    StaticCommittee<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
>;

// TODO (Keyao) Restore code after fixing "overflow evaludating" error.
// impl NodeImplementation<StaticCommitteeTestTypes> for Libp2pImpl {
//     type Storage =
//         MemoryStorage<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>;
//     type Leaf = ValidatingLeaf<StaticCommitteeTestTypes>;
//     type Exchanges = ValidatingExchanges<
//         StaticCommitteeTestTypes,
//         ValidatingLeaf<StaticCommitteeTestTypes>,
//         Message<StaticCommitteeTestTypes, Self, ValidatingMessage<StaticCommitteeTestTypes, Self>>,
//         QuorumExchange<
//             StaticCommitteeTestTypes,
//             ValidatingLeaf<StaticCommitteeTestTypes>,
//             ValidatingProposal<StaticCommitteeTestTypes, ValidatingLeaf<StaticCommitteeTestTypes>>,
//             StaticMembership,
//             StaticCommunication,
//             Message<
//                 StaticCommitteeTestTypes,
//                 Libp2pImpl,
//                 ValidatingMessage<StaticCommitteeTestTypes, Self>,
//             >,
//         >,
//     >;
//     type ConsensusMessage = ValidatingMessage<StaticCommitteeTestTypes, Self>;
// }

// /// libp2p network test
// #[cfg_attr(
//     feature = "tokio-executor",
//     tokio::test(flavor = "multi_thread", worker_threads = 2)
// )]
// #[cfg_attr(feature = "async-std-executor", async_std::test)]
// #[instrument]
// async fn libp2p_network() {
//     let description = GeneralTestDescriptionBuilder::default_multiple_rounds();

//     description
//         .build::<StaticCommitteeTestTypes, Libp2pImpl>()
//         .execute()
//         .await
//         .unwrap();
// }

// // stress test for libp2p
// #[cfg_attr(
//     feature = "tokio-executor",
//     tokio::test(flavor = "multi_thread", worker_threads = 2)
// )]
// #[cfg_attr(feature = "async-std-executor", async_std::test)]
// #[instrument]
// #[ignore]
// async fn test_stress_libp2p_network() {
//     let description = GeneralTestDescriptionBuilder::default_stress();

//     description
//         .build::<StaticCommitteeTestTypes, Libp2pImpl>()
//         .execute()
//         .await
//         .unwrap();
// }
