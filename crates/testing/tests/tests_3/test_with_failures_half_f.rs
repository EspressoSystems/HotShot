// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use hotshot_example_types::{
    node_types::{Libp2pImpl, MemoryImpl, PushCdnImpl, TestVersions},
    state_types::TestTypes,
};
use hotshot_macros::cross_tests;
use hotshot_testing::{
    block_builder::SimpleBuilderImplementation,
    spinning_task::{ChangeNode, SpinningTaskDescription, UpDown},
    test_builder::TestDescription,
};
// Test f/2 nodes leaving the network.
cross_tests!(
    TestName: test_with_failures_half_f,
    Impls: [MemoryImpl, Libp2pImpl, PushCdnImpl],
    Types: [TestTypes],
    Versions: [TestVersions],
    Ignore: false,
    Metadata: {
        let mut metadata = TestDescription::default_more_nodes();
        metadata.num_bootstrap_nodes = 17;
        // The first 14 (i.e., 20 - f) nodes are in the DA committee and we may shutdown the
        // remaining 6 (i.e., f) nodes. We could remove this restriction after fixing the
        // following issue.
        // TODO: Update message broadcasting to avoid hanging
        // <https://github.com/EspressoSystems/HotShot/issues/1567>
        let dead_nodes = vec![
            ChangeNode {
                idx: 17,
                updown: UpDown::Down,
            },
            ChangeNode {
                idx: 18,
                updown: UpDown::Down,
            },
            ChangeNode {
                idx: 19,
                updown: UpDown::Down,
            },
        ];

        metadata.spinning_properties = SpinningTaskDescription {
            node_changes: vec![(5, dead_nodes)]
        };

        metadata.overall_safety_properties.num_failed_views = 3;
        // Make sure we keep committing rounds after the bad leaders, but not the full 50 because of the numerous timeouts
        metadata.overall_safety_properties.num_successful_views = 22;
        metadata
    }
);
