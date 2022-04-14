//! Tests with regarding to querying data between nodes

mod common;

use async_std::task::block_on;
use common::TestDescription;
use common::{
    AppliedTestRunner, TestNetwork, TestRoundResult, TestSafetyCheckPost, TestSafetyCheckPre,
    TestStorage, TestTransaction,
};
use phaselock_testing::ConsensusRoundError;

use either::Either::Right;
use phaselock::{
    traits::NodeImplementation,
    types::{EventType, PhaseLockHandle},
    PhaseLockError,
};
use phaselock_types::traits::storage::Storage;

use snafu::Snafu;
use std::sync::Arc;
use tracing::{error, info};

const NEXT_VIEW_TIMEOUT: u64 = 500;
const DEFAULT_TIMEOUT_RATIO: (u64, u64) = (15, 10);

#[derive(Debug, Snafu)]
enum RoundError {
    PhaseLock { source: PhaseLockError },
}

#[async_std::test]
async fn sync_newest_quorom() {
    let mut checks_pre: TestSafetyCheckPre<TestNetwork, TestStorage> = Vec::new();
    let mut checks_post: TestSafetyCheckPost<TestNetwork, TestStorage> = Vec::new();

    for i in 0..2 {
        let safety_check_pre =
            move |runner: &AppliedTestRunner| -> Result<(), ConsensusRoundError> {
                block_on(async move { validate_qc_numbers(runner.nodes(), i).await });
                Ok(())
            };
        let safety_check_post = move |runner: &AppliedTestRunner,
                                      _results: TestRoundResult|
              -> Result<(), ConsensusRoundError> {
            block_on(async move { validate_qc_numbers(runner.nodes(), i + 1).await });
            Ok(())
        };
        checks_pre.push(Arc::new(safety_check_pre));
        checks_post.push(Arc::new(safety_check_post));
    }

    let setup_round_one = |runner: &mut AppliedTestRunner| -> Vec<TestTransaction> {
        runner
            .add_random_transactions(2)
            .expect("Could not add a random transaction")
    };

    let setup_round_two = |runner: &mut AppliedTestRunner| -> Vec<TestTransaction> {
        block_on(async move {
            let id = runner.add_nodes(1).await[0];
            let mut joiner = runner.get_handle(id).unwrap();
            let first_event = joiner.next_event().await.unwrap();
            match first_event.event {
                EventType::Synced { .. } => {} // ok
                first => {
                    error!("Expected Synced, got {:?}", first,);
                    panic!("FAILED");
                }
            }
            // All nodes should now have QC 1
            validate_qc_numbers(runner.nodes(), 1).await;
            runner
                .add_random_transactions(2)
                .expect("Could not add a random transaction")
        })
    };

    let test_description = TestDescription {
        total_nodes: 5,
        start_nodes: 4,
        num_rounds: 2,
        failure_threshold: 0,
        txn_ids: Right((2, 2)),
        next_view_timeout: NEXT_VIEW_TIMEOUT,
        timeout_ratio: DEFAULT_TIMEOUT_RATIO,
        network_reliability: None,
        setup_round: vec![Arc::new(setup_round_one), Arc::new(setup_round_two)],
        safety_check_pre: checks_pre,
        safety_check_post: checks_post,
        ..TestDescription::default()
    };

    test_description.execute().await.unwrap();
}

async fn validate_qc_numbers<I: NodeImplementation<N>, const N: usize>(
    phaselocks: impl Iterator<Item = &PhaseLockHandle<I, N>>,
    expected: u64,
) {
    for (index, phaselock) in phaselocks.enumerate() {
        let newest_view_number = phaselock
            .storage()
            .get_newest_qc()
            .await
            .unwrap()
            .unwrap()
            .view_number;
        info!(
            "{} is at view number {} (expected {})",
            index, newest_view_number, expected
        );
        assert_eq!(newest_view_number, expected);
    }
}
