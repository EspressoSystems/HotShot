use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};

use hotshot_example_types::{
    node_types::{Libp2pImpl, MarketplaceTestVersions, MemoryImpl, PushCdnImpl, TestVersions},
    state_types::TestTypes,
};
use hotshot_macros::cross_tests;
use hotshot_testing::{
    block_builder::SimpleBuilderImplementation,
    byzantine::byzantine_behaviour::{
        BadProposalViewDos, DishonestDa, DishonestLeader, DoubleProposeVote, ViewDelay,
    },
    completion_task::{CompletionTaskDescription, TimeBasedCompletionTaskDescription},
    test_builder::{Behaviour, TestDescription},
};
use hotshot_types::{data::ViewNumber, traits::node_implementation::ConsensusTime};
use std::rc::Rc;
cross_tests!(
    TestName: double_propose_vote,
    Impls: [MemoryImpl],
    Types: [TestTypes],
    Versions: [TestVersions],
    Ignore: false,
    Metadata: {
        let behaviour = Rc::new(|node_id| { match node_id {
          1 => Behaviour::Byzantine(Box::new(DoubleProposeVote)),
          _ => Behaviour::Standard,
          } });

        TestDescription {
            // allow more time to pass in CI
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                                             TimeBasedCompletionTaskDescription {
                                                 duration: Duration::from_secs(60),
                                             },
                                         ),
            behaviour,
            ..TestDescription::default()
        }
    },
);

// Test where node 4 sends out the correct quorum proposal and additionally spams the network with an extra 99 malformed proposals
cross_tests!(
    TestName: multiple_bad_proposals,
    Impls: [MemoryImpl],
    Types: [TestTypes],
    Versions: [TestVersions],
    Ignore: false,
    Metadata: {
        let behaviour = Rc::new(|node_id| { match node_id {
          4 => Behaviour::Byzantine(Box::new(BadProposalViewDos { multiplier: 100, increment: 1 })),
          _ => Behaviour::Standard,
          } });

        let mut metadata = TestDescription {
            // allow more time to pass in CI
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                                             TimeBasedCompletionTaskDescription {
                                                 duration: Duration::from_secs(60),
                                             },
                                         ),
            behaviour,
            ..TestDescription::default()
        };

        metadata.overall_safety_properties.num_failed_views = 0;

        metadata
    },
);

cross_tests!(
    TestName: dishonest_leader,
    Impls: [MemoryImpl],
    Types: [TestTypes],
    Versions: [TestVersions],
    Ignore: false,
    Metadata: {
        let behaviour = Rc::new(|node_id| {
                let dishonest_leader = DishonestLeader {
                    dishonest_at_proposal_numbers: HashSet::from([2, 3]),
                    validated_proposals: Vec::new(),
                    total_proposals_from_node: 0,
                    view_look_back: 1
                };
                match node_id {
                    2 => Behaviour::Byzantine(Box::new(dishonest_leader)),
                    _ => Behaviour::Standard,
                }
            });

        let mut metadata = TestDescription {
            // allow more time to pass in CI
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                                             TimeBasedCompletionTaskDescription {
                                                 duration: Duration::from_secs(60),
                                             },
                                         ),
            behaviour,
            ..TestDescription::default()
        };

        metadata.overall_safety_properties.num_failed_views = 2;
        metadata.num_nodes_with_stake = 5;
        metadata.overall_safety_properties.expected_views_to_fail = HashMap::from([
            (ViewNumber::new(7), false),
            (ViewNumber::new(12), false)
        ]);
        metadata
    },
);

cross_tests!(
    TestName: dishonest_da,
    Impls: [MemoryImpl, Libp2pImpl, PushCdnImpl],
    Types: [TestTypes],
    Versions: [MarketplaceTestVersions],
    Ignore: false,
    Metadata: {
        let behaviour = Rc::new(|node_id| {
                let dishonest_da = DishonestDa {
                    dishonest_at_da_cert_sent_numbers: HashSet::from([2]),
                    total_da_certs_sent_from_node: 0,
                    total_views_add_to_cert: 4
                };
                match node_id {
                    2 => Behaviour::Byzantine(Box::new(dishonest_da)),
                    _ => Behaviour::Standard,
                }
            });

        let mut metadata = TestDescription {
            // allow more time to pass in CI
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                                             TimeBasedCompletionTaskDescription {
                                                 duration: Duration::from_secs(60),
                                             },
                                         ),
            behaviour,
            ..TestDescription::default()
        };

        metadata.num_nodes_with_stake = 10;
        metadata
    },
);

cross_tests!(
    TestName: view_lagging,
    Impls: [MemoryImpl, Libp2pImpl, PushCdnImpl],
    Types: [TestTypes],
    Versions: [MarketplaceTestVersions],
    Ignore: false,
    Metadata: {
        let num_nodes_with_stake = 10;
        let behaviour = Rc::new(|node_id| {
                let view_delay = ViewDelay {
                    number_of_views_to_delay: 2,
                    received_events: HashMap::new(),
                    vote_rcv_count: HashMap::new(),
                    num_nodes_with_stake: 10,
                    views_to_be_delayed_for: 50,
                    node_id: 2,
                };
                match node_id {
                    8 => Behaviour::Byzantine(Box::new(view_delay)),
                    _ => Behaviour::Standard,
                }
            });

        let mut metadata = TestDescription {
            // allow more time to pass in CI
            completion_task_description: CompletionTaskDescription::TimeBasedCompletionTaskBuilder(
                                             TimeBasedCompletionTaskDescription {
                                                 duration: Duration::from_secs(60),
                                             },
                                         ),
            behaviour,
            ..TestDescription::default()
        };

        metadata.num_nodes_with_stake = num_nodes_with_stake;
        metadata.da_staked_committee_size = num_nodes_with_stake;
        metadata.overall_safety_properties.num_failed_views = 7;
        metadata.overall_safety_properties.expected_views_to_fail = HashMap::from([
            (ViewNumber::new(8), false),
            (ViewNumber::new(18), false),
            (ViewNumber::new(28), false),
            (ViewNumber::new(38), false),
            (ViewNumber::new(48), false),
        ]);
        metadata.overall_safety_properties.num_successful_views = 40;
        metadata
    },
);
