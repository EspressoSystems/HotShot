use hotshot::demos::vdemo::VDemoState;
use hotshot::traits::implementations::{CentralizedCommChannel, MemoryCommChannel, MemoryStorage};
use hotshot::{demos::sdemo::SDemoState, traits::implementations::Libp2pCommChannel};
use hotshot_testing::test_description::GeneralTestDescriptionBuilder;
use hotshot_testing_macros::{cross_all_types, cross_tests};
use hotshot_types::data::ViewNumber;
use hotshot_types::traits::signature_key::ed25519::Ed25519Pub;

cross_tests!(
    DemoType: [ (SequencingConsensus, SDemoState), (ValidatingConsensus, VDemoState) ],
    SignatureKey: [ Ed25519Pub ],
    CommChannel: [ MemoryCommChannel, Libp2pCommChannel, CentralizedCommChannel ],
    Time: [ ViewNumber ],
    TestName: example_test,
    TestDescription: GeneralTestDescriptionBuilder::default(),
    Storage: [ MemoryStorage ],
    Slow: false,
);

cross_all_types!(
    TestName: example_test_2,
    TestDescription: GeneralTestDescriptionBuilder::default(),
    Slow: false,
);
