// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

// TODO: Remove this after integration
#![allow(unused_imports)]
use std::collections::HashMap;

use hotshot_example_types::{
    node_types::{
        CombinedImpl, EpochsTestVersions, Libp2pImpl, MemoryImpl, PushCdnImpl,
        TestConsecutiveLeaderTypes, TestTwoStakeTablesTypes, TestVersions,
    },
    state_types::TestTypes,
};
use hotshot_macros::cross_tests;
use hotshot_testing::{
    block_builder::SimpleBuilderImplementation,
    spinning_task::{ChangeNode, NodeAction, SpinningTaskDescription},
    test_builder::TestDescription,
    view_sync_task::ViewSyncTaskDescription,
};
use hotshot_types::{
    data::ViewNumber,
    message::{GeneralConsensusMessage, MessageKind, SequencingMessage},
    traits::{
        election::Membership,
        network::TransmitType,
        node_implementation::{ConsensusTime, NodeType},
    },
    vote::HasViewNumber,
};

// Test that a good leader can succeed in the view directly after view sync
cross_tests!(
    TestName: test_with_failures_2,
    Impls: [MemoryImpl, Libp2pImpl, PushCdnImpl],
    Types: [TestTypes],
    Versions: [TestVersions],
    Ignore: false,
    Metadata: {
        let mut metadata = TestDescription::default_more_nodes();
        metadata.num_bootstrap_nodes = 10;
        metadata.num_nodes_with_stake = 12;
        metadata.da_staked_committee_size = 12;
        metadata.start_nodes = 12;
        // The first 14 (i.e., 20 - f) nodes are in the DA committee and we may shutdown the
        // remaining 6 (i.e., f) nodes. We could remove this restriction after fixing the
        // following issue.
        let dead_nodes = vec![
            ChangeNode {
                idx: 10,
                updown: NodeAction::Down,
            },
            ChangeNode {
                idx: 11,
                updown: NodeAction::Down,
            },
        ];

        metadata.spinning_properties = SpinningTaskDescription {
            node_changes: vec![(5, dead_nodes)]
        };

        // 2 nodes fail triggering view sync, expect no other timeouts
        metadata.overall_safety_properties.num_failed_views = 2;
        // Make sure we keep committing rounds after the bad leaders, but not the full 50 because of the numerous timeouts
        metadata.overall_safety_properties.num_successful_views = 13;

        metadata
    }
);

cross_tests!(
    TestName: test_with_failures_2_with_epochs,
    Impls: [Libp2pImpl, PushCdnImpl, CombinedImpl],
    Types: [TestTwoStakeTablesTypes],
    Versions: [EpochsTestVersions],
    Ignore: false,
    Metadata: {
        let mut metadata = TestDescription::default_more_nodes();
        metadata.num_nodes_with_stake = 12;
        metadata.da_staked_committee_size = 12;
        metadata.start_nodes = 12;
        let dead_nodes = vec![
            ChangeNode {
                idx: 10,
                updown: NodeAction::Down,
            },
            ChangeNode {
                idx: 11,
                updown: NodeAction::Down,
            },
        ];

        metadata.spinning_properties = SpinningTaskDescription {
            node_changes: vec![(5, dead_nodes)]
        };

        // 2 nodes fail triggering view sync, expect no other timeouts
        metadata.overall_safety_properties.num_failed_views = 6;
        // Make sure we keep committing rounds after the bad leaders, but not the full 50 because of the numerous timeouts
        metadata.overall_safety_properties.num_successful_views = 20;
        metadata.overall_safety_properties.expected_views_to_fail = HashMap::from([
            (ViewNumber::new(5), false),
            (ViewNumber::new(11), false),
            (ViewNumber::new(17), false),
            (ViewNumber::new(23), false),
            (ViewNumber::new(29), false),
            (ViewNumber::new(35), false),
        ]);

        metadata
    }
);

cross_tests!(
    TestName: test_with_double_leader_failures,
    Impls: [MemoryImpl, Libp2pImpl, PushCdnImpl],
    Types: [TestConsecutiveLeaderTypes],
    Versions: [TestVersions],
    Ignore: false,
    Metadata: {
        let mut metadata = TestDescription::default_more_nodes();
        metadata.num_bootstrap_nodes = 10;
        metadata.num_nodes_with_stake = 12;
        metadata.da_staked_committee_size = 12;
        metadata.start_nodes = 12;
        let dead_nodes = vec![
            ChangeNode {
                idx: 3,
                updown: NodeAction::Down,
            },
        ];

        // shutdown while node 3 is leader
        // we want to trigger `ViewSyncTrigger`
        // then ensure we do not fail again as next leader will be leader 2 views also
        let view_spin_node_down = 5;
        metadata.spinning_properties = SpinningTaskDescription {
            node_changes: vec![(view_spin_node_down, dead_nodes)]
        };

        // node 3 is leader twice when we shut down
        metadata.overall_safety_properties.num_failed_views = 2;
        metadata.overall_safety_properties.expected_views_to_fail = HashMap::from([
            // next views after turning node off
            (ViewNumber::new(view_spin_node_down + 1), false),
            (ViewNumber::new(view_spin_node_down + 2), false)
        ]);
        // Make sure we keep committing rounds after the bad leaders, but not the full 50 because of the numerous timeouts
        metadata.overall_safety_properties.num_successful_views = 13;

        // only turning off 1 node, so expected should be num_nodes_with_stake - 1
        let expected_nodes_in_view_sync = metadata.num_nodes_with_stake - 1;
        metadata.view_sync_properties = ViewSyncTaskDescription::Threshold(expected_nodes_in_view_sync, expected_nodes_in_view_sync);

        metadata
    }
);
