mod common;

use std::sync::Arc;

use async_std::task::block_on;
use common::{
    DetailedTestDescriptionBuilder, GeneralTestDescriptionBuilder, TestDescription, TestNetwork,
    TestRoundResult, TestTransaction,
};
use either::Either::Right;
use phaselock_testing::{ConsensusRoundError, Round, TestRunner};

use phaselock::{
    data::{Leaf, QuorumCertificate, StateHash},
    demos::dentry::{
        random_leaf, random_quorom_certificate, random_transaction, DEntryBlock, State as DemoState,
    },
    traits::{BlockContents, Storage},
    PhaseLockConfig, H_256,
};
use phaselock_testing::TestLauncher;
use phaselock_types::{data::ViewNumber, traits::state::TestableState};
use phaselock_utils::test_util::{setup_backtrace, setup_logging};
use rand::thread_rng;
use tracing::debug_span;

type AtomicStorage =
    phaselock::traits::implementations::AtomicStorage<DEntryBlock, DemoState, H_256>;

#[async_std::test]
async fn test_happy_path_blocks() {
    // This folder will be destroyed when the last handle to it closes
    let file = tempfile::tempdir().expect("Could not create temp dir");
    let path = file.path();
    println!("Using store in {:?}", path);
    let mut store = AtomicStorage::open(path).expect("Could not open atomic store");

    let block = DEntryBlock::default();
    let hash = block.hash();
    store
        .update(|mut m| {
            let block = block.clone();
            async move { m.insert_block(hash, block).await }
        })
        .await
        .unwrap();

    // Make sure the data is still there after re-opening
    drop(store);
    store = AtomicStorage::open(path).expect("Could not open atomic store");
    assert_eq!(
        store.get_block(&hash).await.unwrap(),
        Some(DEntryBlock::default())
    );

    // Add some transactions
    let mut rng = thread_rng();
    let state = <DemoState as TestableState<H_256>>::get_starting_state();
    let mut hashes = Vec::new();
    let mut block = block;
    for _ in 0..10 {
        let new = block
            .add_transaction_raw(&random_transaction(&state, &mut rng))
            .expect("Could not add transaction");
        println!("Inserting {:?}: {:?}", new.hash(), new);
        store
            .update(|mut m| {
                let new = new.clone();
                async move { m.insert_block(new.hash(), new.clone()).await }
            })
            .await
            .unwrap();
        hashes.push(new.hash());
        block = new;
    }

    // read them all back 3 times
    // 1st time: normal readback
    // 2nd: after dropping and re-opening the store
    for i in 0..3 {
        if i == 1 {
            drop(store);
            store = AtomicStorage::open(path).expect("Could not open atomic store");
        }

        // read them all back
        for (idx, hash) in hashes.iter().enumerate() {
            match store.get_block(hash).await.expect("Could not read hash") {
                Some(block) => println!("read {:?}", block),
                None => panic!("Could not read hash {} {:?}", idx, hash),
            }
        }
    }
}

#[async_std::test]
async fn test_happy_path_qcs() {
    // This folder will be destroyed when the last handle to it closes
    let file = tempfile::tempdir().expect("Could not create temp dir");
    let path = file.path();
    println!("Using store in {:?}", path);
    let mut store = AtomicStorage::open(path).expect("Could not open atomic store");

    // Add some certificates
    let mut certs = Vec::<QuorumCertificate<H_256>>::new();
    for i in 0..10 {
        let cert = QuorumCertificate {
            view_number: ViewNumber::new(i),
            ..random_quorom_certificate()
        };
        println!("Inserting {:?}", cert);
        store
            .update(|mut m| {
                let cert = cert.clone();
                async move { m.insert_qc(cert).await }
            })
            .await
            .unwrap();
        certs.push(cert);
    }

    // read them all back 3 times
    // 1st time: normal readback
    // 2nd: after dropping and re-opening the store
    for i in 0..2 {
        if i == 1 {
            drop(store);
            store = AtomicStorage::open(path).expect("Could not open atomic store");
        }

        for cert in &certs {
            match store
                .get_qc_for_view(cert.view_number)
                .await
                .expect("Could not read view_number")
            {
                Some(c) => {
                    println!("read {:?}", c);
                    assert_eq!(&c, cert);
                }
                None => panic!("Could not read {:?}: {:?}", cert.view_number, cert),
            }
            match store
                .get_qc(&cert.block_hash)
                .await
                .expect("Could not read qc by hash")
            {
                Some(c) => {
                    println!("read {:?}", c);
                    assert_eq!(&c, cert);
                }
                None => panic!(
                    "Could not read block_hash {:?}: {:?}",
                    cert.block_hash, cert
                ),
            }
        }
    }
}

#[async_std::test]
async fn test_happy_path_leaves() {
    // This folder will be destroyed when the last handle to it closes
    let file = tempfile::tempdir().expect("Could not create temp dir");
    let path = file.path();
    println!("Using store in {:?}", path);
    let mut store = AtomicStorage::open(path).expect("Could not open atomic store");

    // Add some leaves
    let mut leaves = Vec::<Leaf<DEntryBlock, H_256>>::new();
    for _ in 0..10 {
        let leaf = random_leaf(DEntryBlock {
            previous_block: StateHash::random(),
            ..Default::default()
        });
        println!("Inserting {:?}", leaf);
        store
            .update(|mut m| {
                let leaf = leaf.clone();
                async move { m.insert_leaf(leaf).await }
            })
            .await
            .unwrap();
        leaves.push(leaf);
    }

    // read them all back 2 times
    // 1st time: normal readback
    // 2nd: after dropping and re-opening the store
    for i in 0..2 {
        if i == 1 {
            drop(store);
            store = AtomicStorage::open(path).expect("Could not open atomic store");
        }

        for leaf in &leaves {
            match store
                .get_leaf(&leaf.hash())
                .await
                .expect("Could not read leaf hash")
            {
                Some(l) => {
                    println!("read {:?}", l);
                    assert_eq!(&l, leaf);
                }
                None => {
                    panic!("Could not read leaf hash {:?}: {:?}", leaf.hash(), leaf)
                }
            }
            let hash = BlockContents::hash(&leaf.item);
            match store
                .get_leaf_by_block(&hash)
                .await
                .expect("Could not read leaf by block")
            {
                Some(l) => {
                    println!("read {:?}", l);
                    assert_eq!(&l, leaf);
                }
                None => panic!("Could not read leaf hash {:?}: {:?}", hash, leaf),
            }
        }
    }
}

#[async_std::test]
async fn restart() {
    use std::path::Path;

    setup_logging();
    setup_backtrace();

    const PATH: &str = "target/test/restart";
    // make sure PATH doesn't exist
    let _ = async_std::fs::remove_dir_all(PATH).await;

    let round_one_pre_check = |runner: &TestRunner<
        TestNetwork,
        AtomicStorage,
        DEntryBlock,
        DemoState,
    >|
     -> Result<(), ConsensusRoundError> {
        block_on(async move {
            runner.validate_node_states().await;
            // nodes should start at view_number 0
            for node in runner.nodes() {
                assert_eq!(
                    node.storage()
                        .get_newest_qc()
                        .await
                        .unwrap()
                        .unwrap()
                        .view_number,
                    ViewNumber::new(0)
                );
            }
        });
        Ok(())
    };

    let round_one_setup = |runner: &mut TestRunner<
        TestNetwork,
        AtomicStorage,
        DEntryBlock,
        DemoState,
    >|
     -> Vec<TestTransaction> {
        runner.add_random_transactions(1).unwrap()
    };

    let round_one_post_check =
        |runner: &TestRunner<TestNetwork, AtomicStorage, DEntryBlock, DemoState>,
         _results: TestRoundResult|
         -> Result<(), ConsensusRoundError> {
            block_on(async move {
                runner.validate_node_states().await;

                // nodes should now be at view_number 1
                for node in runner.nodes() {
                    assert_eq!(
                        node.storage()
                            .get_newest_qc()
                            .await
                            .unwrap()
                            .unwrap()
                            .view_number,
                        ViewNumber::new(1)
                    );
                }
            });
            Ok(())
        };

    let gen_runner = Arc::new(
        |desc: &TestDescription<TestNetwork, AtomicStorage, DEntryBlock, DemoState>| {
            let launcher = TestLauncher::<TestNetwork, AtomicStorage, DEntryBlock, DemoState>::new(
                desc.total_nodes,
            );

            // modify runner to recognize timing params
            let set_timing_params = |a: &mut PhaseLockConfig| {
                a.next_view_timeout = desc.timing_config.next_view_timeout;
                a.timeout_ratio = desc.timing_config.timeout_ratio;
                a.round_start_delay = desc.timing_config.round_start_delay;
                a.start_delay = desc.timing_config.start_delay;
            };

            // create runner from launcher
            launcher
                // insert timing parameters
                .modify_default_config(set_timing_params)
                .with_storage(|idx| {
                    let span = debug_span!("Storage", idx);
                    let _guard = span;
                    let path = format!("{}/{}", PATH, idx);
                    AtomicStorage::open(Path::new(&path)).unwrap()
                })
                .launch()
        },
    );

    let desc = DetailedTestDescriptionBuilder::<TestNetwork, AtomicStorage, DEntryBlock, DemoState> {
        general_info: GeneralTestDescriptionBuilder {
            total_nodes: 5,
            start_nodes: 5,
            num_succeeds: 1,
            failure_threshold: 0,
            txn_ids: Right(1),
            next_view_timeout: 1000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            ids_to_shut_down: Vec::new(),
            network_reliability: None,
        },
        rounds: Some(vec![Round {
            setup_round: Some(Arc::new(round_one_setup)),
            safety_check_pre: Some(Arc::new(round_one_pre_check)),
            safety_check_post: Some(Arc::new(round_one_post_check)),
        }]),
        gen_runner: Some(gen_runner.clone()),
    };

    desc.build().execute().await.unwrap();

    let round_two_pre_check = |runner: &TestRunner<
        TestNetwork,
        AtomicStorage,
        DEntryBlock,
        DemoState,
    >|
     -> Result<(), ConsensusRoundError> {
        block_on(async move {
            runner.validate_node_states().await;
            // nodes should start at view_number 1
            for node in runner.nodes() {
                assert_eq!(
                    node.storage()
                        .get_newest_qc()
                        .await
                        .unwrap()
                        .unwrap()
                        .view_number,
                    ViewNumber::new(1)
                );
            }
        });
        Ok(())
    };

    let round_two_setup = |runner: &mut TestRunner<
        TestNetwork,
        AtomicStorage,
        DEntryBlock,
        DemoState,
    >|
     -> Vec<TestTransaction> {
        runner.add_random_transactions(1).unwrap()
    };

    let round_two_post_check =
        |runner: &TestRunner<TestNetwork, AtomicStorage, DEntryBlock, DemoState>,
         _results: TestRoundResult|
         -> Result<(), ConsensusRoundError> {
            block_on(async move {
                runner.validate_node_states().await;

                // nodes should now be at view_number 2
                for node in runner.nodes() {
                    assert_eq!(
                        node.storage()
                            .get_newest_qc()
                            .await
                            .unwrap()
                            .unwrap()
                            .view_number,
                        ViewNumber::new(2)
                    );
                }
            });
            Ok(())
        };

    let desc = DetailedTestDescriptionBuilder::<TestNetwork, AtomicStorage, DEntryBlock, DemoState> {
        general_info: GeneralTestDescriptionBuilder {
            total_nodes: 5,
            start_nodes: 5,
            num_succeeds: 1,
            failure_threshold: 0,
            txn_ids: Right(1),
            next_view_timeout: 1000,
            timeout_ratio: (11, 10),
            round_start_delay: 1,
            start_delay: 1,
            ids_to_shut_down: Vec::new(),
            network_reliability: None,
        },
        rounds: Some(vec![Round {
            setup_round: Some(Arc::new(round_two_setup)),
            safety_check_pre: Some(Arc::new(round_two_pre_check)),
            safety_check_post: Some(Arc::new(round_two_post_check)),
        }]),
        gen_runner: Some(gen_runner),
    };
    desc.build().execute().await.unwrap();
}
