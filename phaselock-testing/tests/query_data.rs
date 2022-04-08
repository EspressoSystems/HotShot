//! Tests with regarding to querying data between nodes

mod common;

use async_std::future::{timeout, TimeoutError};
use common::{get_networkings, get_threshold, init_state_and_phaselocks, setup_logging};
use phaselock::{
    demos::dentry::*,
    tc,
    traits::{
        election::StaticCommittee,
        implementations::{DummyReliability, MemoryNetwork},
        NodeImplementation,
    },
    types::{EventType, Message, PhaseLockHandle},
    PhaseLock, PhaseLockConfig, PhaseLockError, PubKey, H_256,
};
use phaselock_types::traits::storage::Storage;
use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use snafu::Snafu;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    time::Duration,
};
use tracing::{debug, error, info};

#[allow(clippy::upper_case_acronyms)]
type NODE = DEntryNode<MemoryNetwork<Message<DEntryBlock, Transaction, State, H_256>>>;

const NEXT_VIEW_TIMEOUT: u64 = 500;
const DEFAULT_TIMEOUT_RATIO: (u64, u64) = (15, 10);
const SEED: u64 = 1234;

fn init_state() -> State {
    // Create the initial state
    let balances: BTreeMap<Account, Balance> = vec![
        ("Joe", 1_000_000),
        ("Nathan M", 500_000),
        ("John", 400_000),
        ("Nathan Y", 600_000),
        ("Ian", 100),
    ]
    .into_iter()
    .map(|(x, y)| (x.to_string(), y))
    .collect();
    State {
        balances,
        nonces: BTreeSet::default(),
    }
}

#[derive(Debug, Snafu)]
enum RoundError {
    PhaseLock { source: PhaseLockError },
}

async fn run_round<I: NodeImplementation<N>, const N: usize>(
    handles: &mut [PhaseLockHandle<I, N>],
    round: usize,
) -> Result<(), RoundError> {
    // Start phaselocks
    for phaselock in handles.iter() {
        phaselock.run_one_round().await;
    }

    // iter through all handles until there are no more events
    let mut any_event = true;
    while any_event {
        any_event = false;
        for (index, handle) in handles.iter_mut().enumerate() {
            match timeout(Duration::from_millis(100), handle.next_event()).await {
                Ok(Ok(event)) => {
                    info!(?round, ?index, ?event);
                    any_event = true;
                }
                Ok(Err(e)) => {
                    error!(?round, ?index, ?e);
                    return Err(RoundError::PhaseLock { source: e });
                }
                Err(TimeoutError { .. }) => {
                    continue;
                }
            }
        }
    }
    Ok(())
}

#[async_std::test]
async fn sync_newest_quorom() {
    setup_logging();

    let num_nodes = 5;
    let threshold = get_threshold(num_nodes);
    let mut rng = Xoshiro256StarStar::seed_from_u64(SEED);
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);

    let (master_network, networkings) =
        get_networkings::<Message<DEntryBlock, Transaction, State, H_256>>(num_nodes, &sks).await;

    debug!("First nodes connected to network");

    let known_nodes: Vec<_> = (0..num_nodes + 1)
        .map(|x| PubKey::from_secret_key_set_escape_hatch(&sks, x))
        .collect();
    // Initialize the state and phaselocks
    let (state, mut phaselocks) = init_state_and_phaselocks::<NODE, H_256>(
        &sks,
        num_nodes,
        known_nodes,
        HashSet::new(),
        threshold,
        networkings,
        DEFAULT_TIMEOUT_RATIO,
        NEXT_VIEW_TIMEOUT,
        init_state(),
    )
    .await;

    info!("Before round 1:");
    validate_qc_numbers(&phaselocks, 0).await;

    // Two nodes propose transactions
    debug!("Proposing two transactions");
    let txn_1 = random_transaction(&state, &mut rng);
    let txn_2 = random_transaction(&state, &mut rng);
    debug!("Txn 1: {:?}\n Txn 2: {:?}", txn_1, txn_2);
    phaselocks[0]
        .submit_transaction(txn_1.clone())
        .await
        .unwrap();
    phaselocks[1]
        .submit_transaction(txn_2.clone())
        .await
        .unwrap();
    debug!("Transactions proposed");

    run_round(&mut phaselocks, 1)
        .await
        .expect("Could not run round");

    info!("After round 1:");
    validate_qc_numbers(&phaselocks, 1).await;

    // Have another node join, this node should get QC view number 1 send to it

    let node_id = num_nodes;
    let pub_key = PubKey::from_secret_key_set_escape_hatch(&sks, node_id);
    let new_network = MemoryNetwork::new(
        pub_key.clone(),
        master_network.clone(),
        Option::<DummyReliability>::None,
    );

    let known_nodes: Vec<_> = (0..num_nodes)
        .map(|x| PubKey::from_secret_key_set_escape_hatch(&sks, x))
        .collect();
    let config = PhaseLockConfig {
        total_nodes: num_nodes as u32,
        threshold: threshold as u32,
        max_transactions: 100,
        known_nodes: known_nodes.clone(),
        next_view_timeout: NEXT_VIEW_TIMEOUT,
        timeout_ratio: DEFAULT_TIMEOUT_RATIO,
        round_start_delay: 1,
        start_delay: 1,
    };
    debug!(?config);
    let mut phaselock = PhaseLock::<NODE, H_256>::init(
        <NODE as NodeImplementation<H_256>>::Block::default(),
        sks.public_keys(),
        sks.secret_key_share(node_id),
        node_id,
        config.clone(),
        state.clone(),
        new_network,
        <NODE as NodeImplementation<H_256>>::Storage::default(),
        <NODE as NodeImplementation<H_256>>::StatefulHandler::default(),
        StaticCommittee::new(known_nodes),
    )
    .await
    .expect("Could not init phaselock");

    // wait for the phaselock to start up

    // These events can come in any order, and `EventType` is not `Ord` (and it shouldn't be)
    // so we have to manually match the two cases
    let first_event = phaselock.next_event().await.unwrap();

    match first_event.event {
        EventType::Synced { .. } => {} // ok
        first => panic!("Expected Synced, got {:?}", first,),
    }

    // All nodes should now have QC 1
    phaselocks.push(phaselock);
    info!("After new node joined:");
    validate_qc_numbers(&phaselocks, 1).await;

    // run the round with this new node connected

    // Two nodes propose transactions
    debug!("Proposing two transactions");
    let txn_1 = random_transaction(&state, &mut rng);
    let txn_2 = random_transaction(&state, &mut rng);
    debug!("Txn 1: {:?}\n Txn 2: {:?}", txn_1, txn_2);
    phaselocks[3]
        .submit_transaction(txn_1.clone())
        .await
        .unwrap();
    phaselocks[node_id as usize]
        .submit_transaction(txn_2.clone())
        .await
        .unwrap();
    debug!("Transactions proposed");

    run_round(&mut phaselocks, 2)
        .await
        .expect("Could not run round");

    info!("After round 2:");
    validate_qc_numbers(&phaselocks, 2).await;
}

async fn validate_qc_numbers<I: NodeImplementation<N>, const N: usize>(
    phaselock: &[PhaseLockHandle<I, N>],
    expected: u64,
) {
    for (index, phaselock) in phaselock.iter().enumerate() {
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
