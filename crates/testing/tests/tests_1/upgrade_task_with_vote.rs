// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

#![allow(unused_imports)]

use std::time::Duration;

use futures::StreamExt;
use hotshot::{tasks::task_state::CreateTaskState, types::SystemContextHandle};
use hotshot_example_types::state_types::TestValidatedState;
use hotshot_example_types::{
    block_types::{TestMetadata, TestTransaction},
    node_types::{MemoryImpl, TestTypes, TestVersions},
    state_types::TestInstanceState,
};
use hotshot_macros::{run_test, test_scripts};
use hotshot_task_impls::{
    consensus::ConsensusTaskState, events::HotShotEvent::*, quorum_vote::QuorumVoteTaskState,
    upgrade::UpgradeTaskState,
};
use hotshot_testing::{
    all_predicates,
    helpers::{build_fake_view_with_leaf, vid_share},
    predicates::{event::*, upgrade_with_vote::*},
    random,
    script::{Expectations, InputOrder, TaskScript},
    view_generator::TestViewGenerator,
};
use hotshot_types::data::Leaf;
use hotshot_types::{
    data::{null_block, ViewNumber},
    simple_vote::UpgradeProposalData,
    traits::{election::Membership, node_implementation::ConsensusTime},
    vote::HasViewNumber,
};
use std::sync::Arc;
use vbs::version::Version;
const TIMEOUT: Duration = Duration::from_millis(65);

#[tokio::test(flavor = "multi_thread")]
/// Tests that we correctly update our internal quorum vote state when reaching a decided upgrade
/// certificate.
async fn test_upgrade_task_with_vote() {
    use hotshot_testing::helpers::build_system_handle;

    hotshot::helpers::initialize_logging();

    let handle = build_system_handle::<TestTypes, MemoryImpl, TestVersions>(2)
        .await
        .0;
    let quorum_membership = handle.hotshot.memberships.quorum_membership.clone();
    let da_membership = handle.hotshot.memberships.da_membership.clone();

    let old_version = Version { major: 0, minor: 1 };
    let new_version = Version { major: 0, minor: 2 };

    let upgrade_data: UpgradeProposalData<TestTypes> = UpgradeProposalData {
        old_version,
        new_version,
        decide_by: ViewNumber::new(6),
        new_version_hash: [0u8; 12].to_vec(),
        old_version_last_view: ViewNumber::new(6),
        new_version_first_view: ViewNumber::new(7),
    };

    let mut proposals = Vec::new();
    let mut votes = Vec::new();
    let mut dacs = Vec::new();
    let mut vids = Vec::new();
    let mut leaders = Vec::new();
    let mut leaves = Vec::new();
    let consensus = handle.hotshot.consensus().clone();
    let mut consensus_writer = consensus.write().await;

    let mut generator = TestViewGenerator::generate(quorum_membership.clone(), da_membership);
    for view in (&mut generator).take(2).collect::<Vec<_>>().await {
        proposals.push(view.quorum_proposal.clone());
        votes.push(view.create_quorum_vote(&handle).await);
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
        leaders.push(view.leader_public_key);
        leaves.push(view.leaf.clone());
        consensus_writer
            .update_leaf(
                Leaf::from_quorum_proposal(&view.quorum_proposal.data),
                Arc::new(TestValidatedState::default()),
                None,
                &handle.hotshot.upgrade_lock,
            )
            .await
            .unwrap();
    }
    drop(consensus_writer);

    generator.add_upgrade(upgrade_data);

    for view in generator.take(4).collect::<Vec<_>>().await {
        proposals.push(view.quorum_proposal.clone());
        votes.push(view.create_quorum_vote(&handle).await);
        dacs.push(view.da_certificate.clone());
        vids.push(view.vid_proposal.clone());
        leaders.push(view.leader_public_key);
        leaves.push(view.leaf.clone());
    }

    let inputs = vec![
        random![
            QuorumProposalValidated(proposals[1].clone(), leaves[0].clone()),
            DaCertificateRecv(dacs[1].clone()),
            VidShareRecv(leaders[1], vids[1].0[0].clone()),
        ],
        random![
            QuorumProposalValidated(proposals[2].clone(), leaves[1].clone()),
            DaCertificateRecv(dacs[2].clone()),
            VidShareRecv(leaders[2], vids[2].0[0].clone()),
        ],
        random![
            QuorumProposalValidated(proposals[3].clone(), leaves[2].clone()),
            DaCertificateRecv(dacs[3].clone()),
            VidShareRecv(leaders[3], vids[3].0[0].clone()),
        ],
        random![
            QuorumProposalValidated(proposals[4].clone(), leaves[3].clone()),
            DaCertificateRecv(dacs[4].clone()),
            VidShareRecv(leaders[4], vids[4].0[0].clone()),
        ],
        random![QuorumProposalValidated(
            proposals[5].clone(),
            leaves[5].clone()
        ),],
    ];

    let expectations = vec![
        Expectations::from_outputs(all_predicates![
            exact(DaCertificateValidated(dacs[1].clone())),
            exact(VidShareValidated(vids[1].0[0].clone())),
            exact(QuorumVoteDependenciesValidated(ViewNumber::new(2))),
            exact(ViewChange(ViewNumber::new(3))),
            quorum_vote_send(),
        ]),
        Expectations::from_outputs_and_task_states(
            all_predicates![
                exact(LockedViewUpdated(ViewNumber::new(1))),
                exact(DaCertificateValidated(dacs[2].clone())),
                exact(VidShareValidated(vids[2].0[0].clone())),
                exact(QuorumVoteDependenciesValidated(ViewNumber::new(3))),
                exact(ViewChange(ViewNumber::new(4))),
                quorum_vote_send(),
            ],
            vec![no_decided_upgrade_certificate()],
        ),
        Expectations::from_outputs_and_task_states(
            all_predicates![
                exact(LockedViewUpdated(ViewNumber::new(2))),
                exact(LastDecidedViewUpdated(ViewNumber::new(1))),
                leaf_decided(),
                exact(DaCertificateValidated(dacs[3].clone())),
                exact(VidShareValidated(vids[3].0[0].clone())),
                exact(QuorumVoteDependenciesValidated(ViewNumber::new(4))),
                exact(ViewChange(ViewNumber::new(5))),
                quorum_vote_send(),
            ],
            vec![no_decided_upgrade_certificate()],
        ),
        Expectations::from_outputs_and_task_states(
            all_predicates![
                exact(LockedViewUpdated(ViewNumber::new(3))),
                exact(LastDecidedViewUpdated(ViewNumber::new(2))),
                leaf_decided(),
                exact(DaCertificateValidated(dacs[4].clone())),
                exact(VidShareValidated(vids[4].0[0].clone())),
                exact(QuorumVoteDependenciesValidated(ViewNumber::new(5))),
                exact(ViewChange(ViewNumber::new(6))),
                quorum_vote_send(),
            ],
            vec![no_decided_upgrade_certificate()],
        ),
        Expectations::from_outputs_and_task_states(
            all_predicates![
                exact(LockedViewUpdated(ViewNumber::new(4))),
                exact(LastDecidedViewUpdated(ViewNumber::new(3))),
                leaf_decided(),
            ],
            vec![decided_upgrade_certificate()],
        ),
    ];

    let vote_state =
        QuorumVoteTaskState::<TestTypes, MemoryImpl, TestVersions>::create_from(&handle).await;
    let mut vote_script = TaskScript {
        timeout: TIMEOUT,
        state: vote_state,
        expectations,
    };

    run_test![inputs, vote_script].await;
}
