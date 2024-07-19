use std::time::Duration;

use hotshot_example_types::{
    node_types::{Libp2pImpl, MemoryImpl, PushCdnImpl},
    state_types::TestTypes,
};
use hotshot_macros::cross_tests;
use hotshot_testing::{
    block_builder::SimpleBuilderImplementation,
    completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
    overall_safety_task::OverallSafetyPropertiesDescription,
    test_builder::TestDescription,
};
cross_tests!(
    TestName: test_success,
    Impls: [MemoryImpl, Libp2pImpl, PushCdnImpl],
    Types: [TestTypes],
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
                num_failed_views: 100,
                ..Default::default()
            },
            ..TestDescription::default()
        }
    },
);
