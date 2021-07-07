#![cfg(feature = "demo")]
mod common;
use common::*;

use rand_xoshiro::{rand_core::SeedableRng, Xoshiro256StarStar};
use tracing::{debug, info, instrument, trace, warn};

use std::collections::VecDeque;

use phaselock::{
    demos::dentry::*,
    event::{Event, EventType},
    handle::PhaseLockHandle,
    message::Message,
    networking::memory_network::{MasterMap, MemoryNetwork},
    tc, PhaseLock, PhaseLockConfig, PubKey, H_256,
};

// This test simulates a single permanent failed node
#[async_std::test]
#[instrument]
async fn single_permanent_failure() {
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
        next_view_timeout: 1_000,
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

    // Simulate the node being failed by taking it out, so it never gets unpaused
    let _failed_node = phaselocks.pop();

    let mut transactions: VecDeque<_> = get_ten_prebaked_trasnactions().into_iter().collect();
    assert_eq!(transactions.len(), 10);
    info!("PhaseLocks prepared, running prebaked transactions");
    let mut round = 1;
    while transactions.len() > 0 {
        let tx = transactions.pop_front().unwrap();
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
        let mut failed = false;
        for (node_id, phaselock) in phaselocks.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State> = phaselock
                .next_event()
                .await
                .expect("PhaseLock unexpectedly closed");
            // Skip all messages from previous rounds
            while event.view_number < round {
                event = phaselock
                    .next_event()
                    .await
                    .expect("PhaseLock unexpectedly closed");
            }
            // Actually wait for decision
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    warn!(?event, "Round timed out!");
                    failed = true;
                    break;
                }
                trace!(?node_id, ?event);
                event = phaselock
                    .next_event()
                    .await
                    .expect("PhaseLock unexpectedly closed");
            }
            if failed {
                // put the tx back where we found it and break
                transactions.push_front(tx.clone());
                break;
            }
            debug!(?node_id, "Node reached decision");
            if let EventType::Decide { block, state } = event.event {
                blocks.push(block);
                states.push(state);
            } else {
                unreachable!()
            }
        }
        if !failed {
            info!("All nodes reached decision");
            assert!(states.len() as u64 == nodes - 1);
            assert!(blocks.len() as u64 == nodes - 1);
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

        // Finally, increment the round counter
        round += 1;
    }
}

// This test simulates two permanent failed nodes
//
// With n=7, this is the maximum failures that the network can tolerate
#[async_std::test]
#[instrument]
async fn double_permanent_failure() {
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
        next_view_timeout: 800,
        timeout_ratio: (15, 10),
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

    // Simulate the nodes being failed by taking them out, so they never gets unpaused
    let _failed_node_1 = phaselocks.pop();
    let _failed_node_2 = phaselocks.pop();

    let mut transactions: VecDeque<_> = get_ten_prebaked_trasnactions().into_iter().collect();
    assert_eq!(transactions.len(), 10);
    info!("PhaseLocks prepared, running prebaked transactions");
    let mut round = 1;
    while transactions.len() > 0 {
        let tx = transactions.pop_front().unwrap();
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
        let mut failed = false;
        for (node_id, phaselock) in phaselocks.iter_mut().enumerate() {
            debug!(?node_id, "Waiting on node to emit decision");
            let mut event: Event<DEntryBlock, State> = phaselock
                .next_event()
                .await
                .expect("PhaseLock unexpectedly closed");
            // Skip all messages from previous rounds
            while event.view_number < round {
                event = phaselock
                    .next_event()
                    .await
                    .expect("PhaseLock unexpectedly closed");
            }
            // Actually wait for decision
            while !matches!(event.event, EventType::Decide { .. }) {
                if matches!(event.event, EventType::ViewTimeout { .. }) {
                    warn!(?event, "Round timed out!");
                    failed = true;
                    break;
                }
                trace!(?node_id, ?event);
                event = phaselock
                    .next_event()
                    .await
                    .expect("PhaseLock unexpectedly closed");
            }
            if failed {
                // put the tx back where we found it and break
                transactions.push_front(tx.clone());
                break;
            }
            debug!(?node_id, "Node reached decision");
            if let EventType::Decide { block, state } = event.event {
                blocks.push(block);
                states.push(state);
            } else {
                unreachable!()
            }
        }
        if !failed {
            info!("All nodes reached decision");
            assert!(states.len() as u64 == nodes - 2);
            assert!(blocks.len() as u64 == nodes - 2);
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

        // Finally, increment the round counter
        round += 1;
    }
}
