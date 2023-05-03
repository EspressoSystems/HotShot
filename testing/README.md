# Purpose

Infrastructure and integration tests for hotshot.

# Usage

The overall control flow is:

```ignore
TestBuilder::default().build() ->  TestLauncher::launch()      -> TestRunner::execute()
|                                  |                              |
- easy override setup fn           - more explicit overrides      - executes the test
|                                    | for networking, storage,
- easy override correctness fn       | hooks/overrides etc
|
- easily add in hooks
|
- easily override launching
```

Easily overriding setup/correctness checks/hooks and launching is all done by anonymous functions. Fairly sane and configurable setup and correct check functions may be generated from the round builder. The intended workflow should look like:
```
use std::sync::Arc;
use futures::FutureExt;
use hotshot_testing::test_types::StaticNodeImplType;
use hotshot_testing::round::RoundHook;
use hotshot_testing::test_types::StaticCommitteeTestTypes;
use hotshot_testing::test_builder::TestBuilder;
use hotshot_testing::test_builder::TestMetadata;

async {
    // specify general characteristics of the test in TestMetadata
    let metadata = TestMetadata {
        total_nodes: 10,
        start_nodes: 10,
        num_succeeds: 5,
        failure_threshold: 10,
        ..Default::default()
    };

    // create normal check and round setup functions based on our spec
    // note: we could override the general setup, hooks, or add in custom safety checks here
    let mut over_ride = Some(TestMetadata::gen_sane_round(&metadata));

    // construct the builder
    // this is meant to be where we can choose to specify "sane" correctness properties under the over_ride attribute
    let mut test_builder = TestBuilder {
        metadata,
        over_ride
    };

    // construct the launcher
    // this may be used to manually override any of the round functions
    let test_launcher = test_builder.build::<StaticCommitteeTestTypes, StaticNodeImplType>();

    /// now let's add in a custom hook to print some debugging information at the beginning
    /// of each view
    let hook =
        RoundHook(Arc::new(move |_runner, ctx| {
            async move {
                tracing::error!("Context for this view is {:#?})", ctx);
                Ok(())
            }
            .boxed_local()
        }));

    /// add the hook, launch the test, then run it.
    test_launcher.push_hook(hook).launch().run_test().await.unwrap();

};
```

See TODO for examples.
