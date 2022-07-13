//! Tests with regarding to querying data between nodes

mod common;

use async_std::task::block_on;
use common::{
    AppliedTestRunner, DetailedTestDescriptionBuilder, GeneralTestDescriptionBuilder,
    TestRoundResult, TestTransaction,
};
use hotshot_testing::{ConsensusRoundError, Round};

use hotshot::{
    traits::NodeImplementation,
    types::{EventType, HotShotHandle},
    HotShotError,
};
use hotshot_types::{data::ViewNumber, traits::storage::Storage};

use snafu::Snafu;
use std::sync::Arc;
use tracing::{error, info};

const NEXT_VIEW_TIMEOUT: u64 = 500;
const DEFAULT_TIMEOUT_RATIO: (u64, u64) = (15, 10);

#[derive(Debug, Snafu)]
enum RoundError {
    HotShot { source: HotShotError },
}

#[ignore]
#[async_std::test]
async fn sync_newest_quorom() {
    let mut rounds = vec![Round::default(); 2];

    for i in 0..2 {
        let safety_check_pre =
            move |runner: &AppliedTestRunner| -> Result<(), ConsensusRoundError> {
                block_on(
                    async move { validate_qc_numbers(runner.nodes(), ViewNumber::new(i)).await },
                );
                Ok(())
            };
        let safety_check_post = move |runner: &AppliedTestRunner,
                                      _results: TestRoundResult|
              -> Result<(), ConsensusRoundError> {
            block_on(
                async move { validate_qc_numbers(runner.nodes(), ViewNumber::new(i + 1)).await },
            );
            Ok(())
        };
        rounds[i as usize].safety_check_pre = Some(Arc::new(safety_check_pre));
        rounds[i as usize].safety_check_post = Some(Arc::new(safety_check_post));
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
            validate_qc_numbers(runner.nodes(), ViewNumber::new(1)).await;
            runner
                .add_random_transactions(2)
                .expect("Could not add a random transaction")
        })
    };

    rounds[0].setup_round = Some(Arc::new(setup_round_one));
    rounds[1].setup_round = Some(Arc::new(setup_round_two));

    let test_description = DetailedTestDescriptionBuilder {
        general_info: GeneralTestDescriptionBuilder {
            total_nodes: 5,
            start_nodes: 4,
            num_succeeds: 2,
            failure_threshold: 0,
            next_view_timeout: NEXT_VIEW_TIMEOUT,
            timeout_ratio: DEFAULT_TIMEOUT_RATIO,
            network_reliability: None,
            ..Default::default()
        },
        rounds: Some(rounds),
        gen_runner: None,
    };

    test_description.build().execute().await.unwrap();
}

async fn validate_qc_numbers<I: NodeImplementation<N>, const N: usize>(
    hotshots: impl Iterator<Item = &HotShotHandle<I, N>>,
    expected: ViewNumber,
) {
    for (index, hotshot) in hotshots.enumerate() {
        let newest_view_number = hotshot
            .storage()
            .get_newest_qc()
            .await
            .unwrap()
            .unwrap()
            .view_number;
        info!(
            "{} is at {:?} (expected {:?})",
            index, newest_view_number, expected
        );
        assert_eq!(newest_view_number, expected);
    }
}
