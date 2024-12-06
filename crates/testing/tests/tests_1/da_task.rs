// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use hotshot::tasks::task_state::CreateTaskState;
use hotshot_example_types::{
    block_types::{TestMetadata, TestTransaction},
    node_types::{MemoryImpl, TestTypes, TestVersions},
};
use hotshot_macros::{run_test, test_scripts};
use hotshot_task_impls::{da::DaTaskState, events::HotShotEvent::*};
use hotshot_testing::{
    helpers::build_system_handle,
    predicates::event::exact,
    script::{Expectations, InputOrder, TaskScript},
    serial,
    view_generator::TestViewGenerator,
};
use hotshot_types::{
    data::{null_block, EpochNumber, PackedBundle, ViewNumber},
    simple_vote::DaData2,
    traits::{
        block_contents::precompute_vid_commitment,
        election::Membership,
        node_implementation::{ConsensusTime, Versions},
    },
};
use vbs::version::StaticVersionType;

#[tokio::test(flavor = "multi_thread")]
async fn test_da_task() {
    hotshot::helpers::initialize_logging();

    let handle = build_system_handle::<TestTypes, MemoryImpl, TestVersions>(2)
        .await
        .0;

    let membership = (*handle.hotshot.memberships).clone();

    // Make some empty encoded transactions, we just care about having a commitment handy for the
    // later calls. We need the VID commitment to be able to propose later.
    let transactions = vec![TestTransaction::new(vec![0])];
    let encoded_transactions = Arc::from(TestTransaction::encode(&transactions));
    let (payload_commit, precompute) = precompute_vid_commitment(
        &encoded_transactions,
        handle.hotshot.memberships.total_nodes(EpochNumber::new(0)),
    );

    let mut generator = TestViewGenerator::generate(membership.clone());

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();

    for view in (&mut generator).take(1).collect::<Vec<_>>().await {
        proposals.push(view.da_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(
            view.create_da_vote(
                DaData2 {
                    payload_commit,
                    epoch: EpochNumber::new(0),
                },
                &handle,
            )
            .await,
        );
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }

    generator.add_transactions(vec![TestTransaction::new(vec![0])]);

    for view in (&mut generator).take(1).collect::<Vec<_>>().await {
        proposals.push(view.da_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(
            view.create_da_vote(
                DaData2 {
                    payload_commit,
                    epoch: EpochNumber::new(0),
                },
                &handle,
            )
            .await,
        );
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }

    let inputs = vec![
        serial![
            ViewChange(ViewNumber::new(1), EpochNumber::new(0)),
            ViewChange(ViewNumber::new(2), EpochNumber::new(0)),
            BlockRecv(PackedBundle::new(
                encoded_transactions.clone(),
                TestMetadata {
                    num_transactions: transactions.len() as u64
                },
                ViewNumber::new(2),
                EpochNumber::new(0),
                vec1::vec1![null_block::builder_fee::<TestTypes, TestVersions>(
                    membership.total_nodes(EpochNumber::new(0)),
                    <TestVersions as Versions>::Base::VERSION,
                    *ViewNumber::new(2),
                )
                .unwrap()],
                Some(precompute),
                None,
            )),
        ],
        serial![DaProposalRecv(proposals[1].clone(), leaders[1])],
    ];

    let da_state = DaTaskState::<TestTypes, MemoryImpl, TestVersions>::create_from(&handle).await;
    let mut da_script = TaskScript {
        timeout: Duration::from_millis(35),
        state: da_state,
        expectations: vec![
            Expectations::from_outputs(vec![exact(DaProposalSend(
                proposals[1].clone(),
                leaders[1],
            ))]),
            Expectations::from_outputs(vec![
                exact(DaProposalValidated(proposals[1].clone(), leaders[1])),
                exact(DaVoteSend(votes[1].clone())),
            ]),
        ],
    };

    run_test![inputs, da_script].await;
}

#[tokio::test(flavor = "multi_thread")]
async fn test_da_task_storage_failure() {
    hotshot::helpers::initialize_logging();

    let handle = build_system_handle::<TestTypes, MemoryImpl, TestVersions>(2)
        .await
        .0;

    // Set the error flag here for the system handle. This causes it to emit an error on append.
    handle.storage().write().await.should_return_err = true;
    let membership = (*handle.hotshot.memberships).clone();

    // Make some empty encoded transactions, we just care about having a commitment handy for the
    // later calls. We need the VID commitment to be able to propose later.
    let transactions = vec![TestTransaction::new(vec![0])];
    let encoded_transactions = Arc::from(TestTransaction::encode(&transactions));
    let (payload_commit, precompute) = precompute_vid_commitment(
        &encoded_transactions,
        handle.hotshot.memberships.total_nodes(EpochNumber::new(0)),
    );

    let mut generator = TestViewGenerator::generate(membership.clone());

    let mut proposals = Vec::new();
    let mut leaders = Vec::new();
    let mut votes = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();

    for view in (&mut generator).take(1).collect::<Vec<_>>().await {
        proposals.push(view.da_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(
            view.create_da_vote(
                DaData2 {
                    payload_commit,
                    epoch: EpochNumber::new(0),
                },
                &handle,
            )
            .await,
        );
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }

    generator.add_transactions(transactions.clone());

    for view in (&mut generator).take(1).collect::<Vec<_>>().await {
        proposals.push(view.da_proposal.clone());
        leaders.push(view.leader_public_key);
        votes.push(
            view.create_da_vote(
                DaData2 {
                    payload_commit,
                    epoch: EpochNumber::new(0),
                },
                &handle,
            )
            .await,
        );
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
    }

    let inputs = vec![
        serial![
            ViewChange(ViewNumber::new(1), EpochNumber::new(0)),
            ViewChange(ViewNumber::new(2), EpochNumber::new(0)),
            BlockRecv(PackedBundle::new(
                encoded_transactions.clone(),
                TestMetadata {
                    num_transactions: transactions.len() as u64
                },
                ViewNumber::new(2),
                EpochNumber::new(0),
                vec1::vec1![null_block::builder_fee::<TestTypes, TestVersions>(
                    membership.total_nodes(EpochNumber::new(0)),
                    <TestVersions as Versions>::Base::VERSION,
                    *ViewNumber::new(2),
                )
                .unwrap()],
                Some(precompute),
                None,
            ),)
        ],
        serial![DaProposalRecv(proposals[1].clone(), leaders[1])],
        serial![DaProposalValidated(proposals[1].clone(), leaders[1])],
    ];
    let expectations = vec![
        Expectations::from_outputs(vec![exact(DaProposalSend(
            proposals[1].clone(),
            leaders[1],
        ))]),
        Expectations::from_outputs(vec![exact(DaProposalValidated(
            proposals[1].clone(),
            leaders[1],
        ))]),
        Expectations::from_outputs(vec![]),
    ];

    let da_state = DaTaskState::<TestTypes, MemoryImpl, TestVersions>::create_from(&handle).await;
    let mut da_script = TaskScript {
        timeout: Duration::from_millis(35),
        state: da_state,
        expectations,
    };

    run_test![inputs, da_script].await;
}
