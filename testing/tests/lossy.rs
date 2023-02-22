#![allow(clippy::type_complexity)]
mod common;

use std::sync::Arc;

use common::{AppliedTestRunner, StaticCommitteeTestTypes, StaticNodeImplType};
use either::Either::Right;
use futures::{future::LocalBoxFuture, FutureExt};
use hotshot_testing::{
    network_reliability::{AsynchronousNetwork, PartiallySynchronousNetwork, SynchronousNetwork},
    test_description::{DetailedTestDescriptionBuilder, GeneralTestDescriptionBuilder},
    ConsensusRoundError, RoundResult,
};
use hotshot_types::data::TestableLeaf;
use hotshot_types::traits::{
    network::TestableNetworkingImplementation,
    node_implementation::{NodeImplementation, NodeType, TestableNodeImplementation},
    signature_key::TestableSignatureKey,
    state::{TestableBlock, TestableState},
    storage::TestableStorage,
};
use tracing::{error, instrument};
/// checks safety requirement; relatively lax
/// marked as success if 2f+1 nodes "succeeded" and committed the same thing
pub fn check_safety<TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
    runner: &AppliedTestRunner<
        TYPES,
        <I as NodeImplementation<TYPES>>::Leaf,
        <I as NodeImplementation<TYPES>>::Proposal,
        <I as NodeImplementation<TYPES>>::Vote,
        <I as NodeImplementation<TYPES>>::Membership,
    >,
    results: RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>,
) -> LocalBoxFuture<Result<(), ConsensusRoundError>>
where
    TYPES::SignatureKey: TestableSignatureKey,
    TYPES::BlockType: TestableBlock,
    TYPES::StateType: TestableState<BlockType = TYPES::BlockType>,
    I::Networking: TestableNetworkingImplementation<TYPES, I::Proposal, I::Vote, I::Membership>,
    I::Storage: TestableStorage<TYPES, I::Leaf>,
    I::Leaf: TestableLeaf<NodeType = TYPES>,
{
    async move {
        let num_nodes = runner.ids().len();
        if results.results.len() <= (2 * num_nodes) / 3 + 1 {
            return Err(ConsensusRoundError::TimedOutWithoutAnyLeader);
        }
        let (first_node_idx, (first_states, first_blocks)) = results.results.iter().next().unwrap();

        for (i_idx, (i_states, i_blocks)) in results.results.clone() {
            // first block/state most recent
            if first_blocks.get(0) != i_blocks.get(0) || first_states.get(0) != i_states.get(0) {
                error!(
                    ?first_blocks,
                    ?i_blocks,
                    ?first_states,
                    ?i_states,
                    ?first_node_idx,
                    ?i_idx,
                    "SAFETY ERROR: most recent block or state does not match"
                );
                panic!("safety check failed");
            }
        }
        Ok(())
    }
    .boxed_local()
}

// tests base level of working synchronous network
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn test_no_loss_network() {
    let description =
        DetailedTestDescriptionBuilder::<StaticCommitteeTestTypes, StaticNodeImplType> {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 10,
                start_nodes: 10,
                network_reliability: Some(Arc::new(SynchronousNetwork::default())),
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    let mut test = description.build();
    test.rounds[0].safety_check_post = Some(Box::new(
        check_safety::<StaticCommitteeTestTypes, StaticNodeImplType>,
    ));
    test.execute().await.unwrap();
}

// // tests network with forced packet delay
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn test_synchronous_network() {
    let description =
        DetailedTestDescriptionBuilder::<StaticCommitteeTestTypes, StaticNodeImplType> {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 5,
                start_nodes: 5,
                num_succeeds: 2,
                txn_ids: Right(1),
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    let mut test = description.build();
    test.rounds[0].safety_check_post = Some(Box::new(
        check_safety::<StaticCommitteeTestTypes, StaticNodeImplType>,
    ));
    test.rounds[1].safety_check_post = Some(Box::new(
        check_safety::<StaticCommitteeTestTypes, StaticNodeImplType>,
    ));
    test.execute().await.unwrap();
}

// tests network with small packet delay and dropped packets
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_asynchronous_network() {
    let description =
        DetailedTestDescriptionBuilder::<StaticCommitteeTestTypes, StaticNodeImplType> {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 5,
                start_nodes: 5,
                num_succeeds: 2,
                txn_ids: Right(1),
                failure_threshold: 5,
                network_reliability: Some(Arc::new(AsynchronousNetwork::new(97, 100, 0, 5))),
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    let mut test = description.build();
    test.rounds[0].safety_check_post = Some(Box::new(
        check_safety::<StaticCommitteeTestTypes, StaticNodeImplType>,
    ));
    test.rounds[1].safety_check_post = Some(Box::new(
        check_safety::<StaticCommitteeTestTypes, StaticNodeImplType>,
    ));
    test.execute().await.unwrap();
}

/// tests network with asynchronous patch that eventually becomes synchronous
#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
#[ignore]
async fn test_partially_synchronous_network() {
    let asn = AsynchronousNetwork::new(90, 100, 0, 0);
    let sn = SynchronousNetwork::new(10, 0);
    let gst = std::time::Duration::new(10, 0);

    let description =
        DetailedTestDescriptionBuilder::<StaticCommitteeTestTypes, StaticNodeImplType> {
            general_info: GeneralTestDescriptionBuilder {
                total_nodes: 5,
                start_nodes: 5,
                num_succeeds: 2,
                txn_ids: Right(1),
                network_reliability: Some(Arc::new(PartiallySynchronousNetwork::new(asn, sn, gst))),
                ..GeneralTestDescriptionBuilder::default()
            },
            rounds: None,
            gen_runner: None,
        };
    let mut test = description.build();
    test.rounds[0].safety_check_post = Some(Box::new(
        check_safety::<StaticCommitteeTestTypes, StaticNodeImplType>,
    ));
    test.rounds[1].safety_check_post = Some(Box::new(
        check_safety::<StaticCommitteeTestTypes, StaticNodeImplType>,
    ));
    test.execute().await.unwrap();
}
