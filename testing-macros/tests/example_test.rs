use hotshot_testing_macros::cross_tests;
use hotshot_types::data::ViewNumber;
use hotshot::demos::sdemo::{SDemoBlock, SDemoState, SDemoTransaction};
use hotshot::demos::vdemo::{VDemoBlock, VDemoState, VDemoTransaction};
use hotshot_types::traits::signature_key::ed25519::Ed25519Pub;
use hotshot::traits::implementations::{MemoryCommChannel, MemoryStorage};
use hotshot_testing::test_description::GeneralTestDescriptionBuilder;

cross_tests!(
    Time: [ ViewNumber ],
    DemoType: [ (SequencingConsensus, SDemoState), (ValidatingConsensus, VDemoState) ],
    SignatureKey: [ Ed25519Pub ],
    CommChannel: [ MemoryCommChannel ],
    Storage: [ MemoryStorage ],
    TestName: example_test2,
    TestDescription: GeneralTestDescriptionBuilder::default(),
    Slow: false,
);
