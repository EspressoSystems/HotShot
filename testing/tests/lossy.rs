#![allow(clippy::type_complexity)]
use std::sync::Arc;

use either::Either::{self, Right};
use futures::{future::LocalBoxFuture, FutureExt};
use hotshot::traits::TestableNodeImplementation;

use hotshot_testing::{
    network_reliability::{AsynchronousNetwork, PartiallySynchronousNetwork, SynchronousNetwork},
    test_builder::{
        TestMetadata, TestMetadata, RoundCheckBuilder,
    },
    test_types::{AppliedTestRunner, StaticCommitteeTestTypes, StaticNodeImplType},
    ConsensusFailedError, RoundCtx, RoundPostSafetyCheck, RoundResult,
};

use hotshot_types::traits::node_implementation::{NodeImplementation, NodeType};
use tracing::{error, instrument};

/// checks safety requirement; relatively lax
/// marked as success if 2f+1 nodes "succeeded" and committed the same thing
pub fn check_safety<'a, TYPES: NodeType, I: TestableNodeImplementation<TYPES>>(
    runner: &'a AppliedTestRunner<TYPES, I>,
    _ctx: &'a mut RoundCtx<TYPES, I>,
    results: RoundResult<TYPES, <I as NodeImplementation<TYPES>>::Leaf>,
) -> LocalBoxFuture<'a, Result<(), ConsensusFailedError>> {
    async move {
        let num_nodes = runner.ids().len();
        if results.success_nodes.len() <= (2 * num_nodes) / 3 + 1 {
            return Err(ConsensusFailedError::TimedOutWithoutAnyLeader);
        }
        let (first_node_idx, (first_states, first_blocks)) = results.success_nodes.iter().next().unwrap();

        for (i_idx, (i_states, i_blocks)) in results.success_nodes.clone() {
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
        TestMetadata::<StaticCommitteeTestTypes, StaticNodeImplType> {
            metadata: TestMetadata {
                total_nodes: 10,
                start_nodes: 10,
                network_reliability: Some(Arc::new(SynchronousNetwork::default())),
                ..TestMetadata::default()
            },
            round: Either::Right(RoundCheckBuilder::default()),
            gen_runner: None,
        };
    let mut test = description.build();
    test.round.safety_check_post = RoundPostSafetyCheck(Arc::new(
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
        TestMetadata::<StaticCommitteeTestTypes, StaticNodeImplType> {
            metadata: TestMetadata {
                total_nodes: 5,
                start_nodes: 5,
                num_succeeds: 2,
                txn_ids: Right(1),
                ..TestMetadata::default()
            },
            round: Either::Right(RoundCheckBuilder::default()),
            gen_runner: None,
        };
    let mut test = description.build();
    test.round.safety_check_post = RoundPostSafetyCheck(Arc::new(
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
        TestMetadata::<StaticCommitteeTestTypes, StaticNodeImplType> {
            metadata: TestMetadata {
                total_nodes: 5,
                start_nodes: 5,
                num_succeeds: 2,
                txn_ids: Right(1),
                failure_threshold: 5,
                network_reliability: Some(Arc::new(AsynchronousNetwork::new(97, 100, 0, 5))),
                ..TestMetadata::default()
            },
            round: Either::Right(RoundCheckBuilder::default()),
            gen_runner: None,
        };
    let mut test = description.build();
    test.round.safety_check_post = RoundPostSafetyCheck(Arc::new(
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
        TestMetadata::<StaticCommitteeTestTypes, StaticNodeImplType> {
            metadata: TestMetadata {
                total_nodes: 5,
                start_nodes: 5,
                num_succeeds: 2,
                txn_ids: Right(1),
                network_reliability: Some(Arc::new(PartiallySynchronousNetwork::new(asn, sn, gst))),
                ..TestMetadata::default()
            },
            round: Either::Right(RoundCheckBuilder::default()),
            gen_runner: None,
        };
    let mut test = description.build();
    test.round.safety_check_post = RoundPostSafetyCheck(Arc::new(
        check_safety::<StaticCommitteeTestTypes, StaticNodeImplType>,
    ));
    test.execute().await.unwrap();
}
