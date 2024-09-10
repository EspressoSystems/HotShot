// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::time::Duration;

use hotshot_example_types::{
    node_types::{MarketplaceTestVersions, MarketplaceUpgradeTestVersions, MemoryImpl},
    state_types::TestTypes,
};
use hotshot_macros::cross_tests;
use hotshot_testing::{
    block_builder::SimpleBuilderImplementation,
    completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
    overall_safety_task::OverallSafetyPropertiesDescription,
    test_builder::{nonempty_block_threshold, TestDescription},
};

cross_tests!(
    TestName: test_success_marketplace_solver_down,
    Impls: [MemoryImpl],
    Types: [TestTypes],
    Versions: [MarketplaceTestVersions],
    Ignore: false,
    Metadata: {
        TestDescription {
            // allow more time to pass in CI
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                                             TimeBasedCompletionTaskDescription {
                                                 duration: Duration::from_secs(60),
                                             },
                                         ),
            overall_safety_properties: OverallSafetyPropertiesDescription {
                transaction_threshold: 0,
                ..OverallSafetyPropertiesDescription::default()
            },
            validate_transactions: nonempty_block_threshold((95,100)),
            start_solver: false,
            ..TestDescription::default()
        }
    },
);

cross_tests!(
    TestName: test_success_marketplace,
    Impls: [MemoryImpl],
    Types: [TestTypes],
    Versions: [MarketplaceUpgradeTestVersions],
    Ignore: false,
    Metadata: {
        TestDescription {
            // allow more time to pass in CI
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                                             TimeBasedCompletionTaskDescription {
                                                 duration: Duration::from_secs(60),
                                             },
                                         ),
            upgrade_view: Some(5),
            validate_transactions: nonempty_block_threshold((40,50)),
            ..TestDescription::default()
        }
    },
);
