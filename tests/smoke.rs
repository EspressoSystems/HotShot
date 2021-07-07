#![cfg(feature = "demo")]
mod common;
use common::*;

use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use tracing::{debug, error, info, instrument, trace};

use phaselock::{
    demos::dentry::*,
    event::{Event, EventType},
    handle::PhaseLockHandle,
    message::Message,
    networking::memory_network::{MasterMap, MemoryNetwork},
    tc, PhaseLock, PhaseLockConfig, PubKey, H_256,
};

#[async_std::test]
#[instrument]
async fn ten_tx_seven_nodes() {
    setup_logging();

    // Calculate the threshold
    let nodes = 7;
    let threshold = ((nodes * 2) / 3) + 1;
    info!(?nodes, ?threshold);
    // Generate the private key set
    // Generated using xoshiro for reproduceability
    let mut rng = Xoshiro256StarStar::seed_from_u64(0);
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);
    // Generate the networking backends
    let master = MasterMap::<Message<DEntryBlock, Transaction, H_256>>::new();
    let mut networkings: Vec<(
        MemoryNetwork<Message<DEntryBlock, Transaction, H_256>>,
        PubKey,
    )> = Vec::new();
    for node_id in 0..nodes {
        let pub_key = PubKey::from_secret_key_set_escape_hatch(&sks, node_id);
        let mn = MemoryNetwork::new(pub_key.clone(), master.clone());
        networkings.push((mn, pub_key));
    }
    info!("Created networking");
    // Create the phaselocks
    let known_nodes: Vec<PubKey> = networkings.iter().map(|(_, x)| x.clone()).collect();
    let config = PhaseLockConfig {
        total_nodes: nodes as u32,
        thershold: threshold as u32,
        max_transactions: 100,
        known_nodes,
        next_view_timeout: 10_000,
        timeout_ratio: (11, 10),
    };
    debug!(?config);
    let gensis = DEntryBlock::default();
    let state = get_starting_state();
    let mut phaselocks: Vec<PhaseLockHandle<_, H_256>> = Vec::new();
    for node_id in 0..nodes {
        let (_, h) = PhaseLock::init(
            gensis.clone(),
            &sks,
            node_id,
            config.clone(),
            state.clone(),
            networkings[node_id as usize].0.clone(),
        )
        .await;
        phaselocks.push(h);
    }

    let transactions = get_ten_prebaked_trasnactions();
    assert_eq!(transactions.len(), 10);
    info!("PhaseLocks prepared, running prebaked transactions");
    for (round, tx) in transactions.into_iter().enumerate() {
        info!(?round, ?tx);
        phaselocks[0]
            .submit_transaction(tx.clone())
            .await
            .expect("Failed to submit transaction");
        info!("Transaction submitted, unlocking round");
        for phaselock in &phaselocks {
            phaselock.run_one_round().await;
        }
        debug!("Waiting for consensus to occur");
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        for (node_id, phaselock) in phaselocks.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State> = phaselock
                .next_event()
                .await
                .expect("PhaseLock unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    error!(?event, "Round timed out!");
                    panic!("Round failed");
                }
                trace!(?node_id, ?event);
                event = phaselock
                    .next_event()
                    .await
                    .expect("PhaseLock unexpectedly closed");
            }
            debug!(?node_id, "Node reached decision");
            if let EventType::Decide { block, state } = event.event {
                blocks.push(block);
                states.push(state);
            } else {
                unreachable!()
            }
        }
        info!("All nodes reached decision");
        assert!(states.len() as u64 == nodes);
        assert!(blocks.len() as u64 == nodes);
        let b_test = &blocks[0];
        for b in &blocks[1..] {
            assert!(b == b_test);
        }
        let s_test = &states[0];
        for s in &states[1..] {
            assert!(s == s_test);
        }
        info!("All states match");
        trace!(state = ?states[0], block = ?blocks[0]);
        assert_eq!(blocks[0].transactions.len(), 1);
        assert_eq!(blocks[0].transactions, vec![tx])
    }
}

#[async_std::test]
#[instrument]
async fn ten_tx_five_nodes() {
    setup_logging();

    // Calculate the threshold
    let nodes = 5;
    let threshold = ((nodes * 2) / 3) + 1;
    info!(?nodes, ?threshold);
    // Generate the private key set
    // Generated using xoshiro for reproduceability
    let mut rng = Xoshiro256StarStar::seed_from_u64(0);
    let sks = tc::SecretKeySet::random(threshold as usize - 1, &mut rng);
    // Generate the networking backends
    let master = MasterMap::<Message<DEntryBlock, Transaction, H_256>>::new();
    let mut networkings: Vec<(
        MemoryNetwork<Message<DEntryBlock, Transaction, H_256>>,
        PubKey,
    )> = Vec::new();
    for node_id in 0..nodes {
        let pub_key = PubKey::from_secret_key_set_escape_hatch(&sks, node_id);
        let mn = MemoryNetwork::new(pub_key.clone(), master.clone());
        networkings.push((mn, pub_key));
    }
    info!("Created networking");
    // Create the phaselocks
    let known_nodes: Vec<PubKey> = networkings.iter().map(|(_, x)| x.clone()).collect();
    let config = PhaseLockConfig {
        total_nodes: nodes as u32,
        thershold: threshold as u32,
        max_transactions: 100,
        known_nodes,
        next_view_timeout: 10_000,
        timeout_ratio: (11, 10),
    };
    debug!(?config);
    let gensis = DEntryBlock::default();
    let state = get_starting_state();
    let mut phaselocks: Vec<PhaseLockHandle<_, H_256>> = Vec::new();
    for node_id in 0..nodes {
        let (_, h) = PhaseLock::init(
            gensis.clone(),
            &sks,
            node_id,
            config.clone(),
            state.clone(),
            networkings[node_id as usize].0.clone(),
        )
        .await;
        phaselocks.push(h);
    }

    let transactions = get_ten_prebaked_trasnactions();
    assert_eq!(transactions.len(), 10);
    info!("PhaseLocks prepared, running prebaked transactions");
    for (round, tx) in transactions.into_iter().enumerate() {
        info!(?round, ?tx);
        phaselocks[0]
            .submit_transaction(tx.clone())
            .await
            .expect("Failed to submit transaction");
        info!("Transaction submitted, unlocking round");
        for phaselock in &phaselocks {
            phaselock.run_one_round().await;
        }
        debug!("Waiting for consensus to occur");
        let mut blocks = Vec::new();
        let mut states = Vec::new();
        for (node_id, phaselock) in phaselocks.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State> = phaselock
                .next_event()
                .await
                .expect("PhaseLock unexpectedly closed");
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    error!(?event, "Round timed out!");
                    panic!("Round failed");
                }
                trace!(?node_id, ?event);
                event = phaselock
                    .next_event()
                    .await
                    .expect("PhaseLock unexpectedly closed");
            }
            debug!(?node_id, "Node reached decision");
            if let EventType::Decide { block, state } = event.event {
                blocks.push(block);
                states.push(state);
            } else {
                unreachable!()
            }
        }
        info!("All nodes reached decision");
        assert!(states.len() as u64 == nodes);
        assert!(blocks.len() as u64 == nodes);
        let b_test = &blocks[0];
        for b in &blocks[1..] {
            assert!(b == b_test);
        }
        let s_test = &states[0];
        for s in &states[1..] {
            assert!(s == s_test);
        }
        info!("All states match");
        trace!(state = ?states[0], block = ?blocks[0]);
        assert_eq!(blocks[0].transactions.len(), 1);
        assert_eq!(blocks[0].transactions, vec![tx])
    }
}
