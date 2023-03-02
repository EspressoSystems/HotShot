use hotshot_testing_macros::cross_tests;

// cross_tests!(
//     ConsensusType: [ ValidatingConsensus, SequencingConsensus ],
//     Time: [ ViewNumber ],
//     DemoType: [ (SDemoBlock, SDemoState, SDemoTransaction), (VDemoBlock, VDemoState, VDemoTransaction) ],
//     SignatureKey: [ Ed25519Pub ],
//     Vote: [ (StaticVoteToken, StaticElectionConfig) ],
//     TestName: example_test,
//     TestDescription: GeneralTestDescriptionBuilder::default(),
//     slow: $slow,
// );

cross_tests!(
    ConsensusType: [ hotshot_types::traits::state::ValidatingConsensus, hotshot_types::traits::state::SequencingConsensus ],
    Time: [ hotshot_types::data::ViewNumber ],
    DemoType: [ (hotshot::demos::sdemo::SDemoBlock, hotshot::demos::sdemo::SDemoState, hotshot::demos::sdemo::SDemoTransaction), (hotshot::demos::vdemo::VDemoBlock, hotshot::demos::vdemo::VDemoState, hotshot::demos::vdemo::VDemoTransaction) ],
    SignatureKey: [ hotshot_types::traits::signature_key::ed25519::Ed25519Pub ],
    Vote: [ (hotshot::traits::election::static_committee::StaticVoteToken, hotshot::traits::election::static_committee::StaticElectionConfig) ],
    TestName: example_test,
    TestDescription: GeneralTestDescriptionBuilder::default(),
    Slow: false,
);
