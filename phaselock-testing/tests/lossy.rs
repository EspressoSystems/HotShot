#![allow(clippy::type_complexity)]
mod common;

use std::sync::Arc;

use common::{
    AppliedTestRunner, DetailedTestDescriptionBuilder, GeneralTestDescriptionBuilder,
    TestRoundResult,
};
use either::Either::Right;
use phaselock_testing::{
    network_reliability::{AsynchronousNetwork, PartiallySynchronousNetwork, SynchronousNetwork},
    ConsensusRoundError,
};
use tracing::{error, instrument, warn};

/// checks safety requirement; relatively lax
/// marked as success if 2f+1 nodes "succeeded" and committed the same thing
pub fn check_safety(
    runner: &AppliedTestRunner,
    results: TestRoundResult,
) -> Result<(), ConsensusRoundError> {
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

// tests base level of working synchronous network
#[async_std::test]
#[instrument]
async fn test_no_loss_network() {
    let description = DetailedTestDescriptionBuilder {
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
    test.rounds[0].safety_check_post = Some(Arc::new(check_safety));
    test.execute().await.unwrap();
}

// // tests network with forced packet delay
#[async_std::test]
#[instrument]
async fn test_synchronous_network() {
    let description = DetailedTestDescriptionBuilder {
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
    test.rounds[0].safety_check_post = Some(Arc::new(check_safety));
    test.rounds[1].safety_check_post = Some(Arc::new(check_safety));
    test.execute().await.unwrap();
}

// tests network with small packet delay and dropped packets
#[async_std::test]
#[instrument]
#[ignore]
async fn test_asynchronous_network() {
    let description = DetailedTestDescriptionBuilder {
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
    test.rounds[0].safety_check_post = Some(Arc::new(check_safety));
    test.rounds[1].safety_check_post = Some(Arc::new(check_safety));
    test.execute().await.unwrap();
}

/// tests network with asynchronous patch that eventually becomes synchronous
#[async_std::test]
#[instrument]
#[ignore]
async fn test_partially_synchronous_network() {
    let asn = AsynchronousNetwork::new(90, 100, 0, 0);
    let sn = SynchronousNetwork::new(10, 0);
    let gst = std::time::Duration::new(10, 0);

    let description = DetailedTestDescriptionBuilder {
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
    test.rounds[0].safety_check_post = Some(Arc::new(check_safety));
    test.rounds[1].safety_check_post = Some(Arc::new(check_safety));
    test.execute().await.unwrap();
}
