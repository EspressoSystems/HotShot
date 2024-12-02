(function() {
    var implementors = Object.fromEntries([["hotshot",[["impl&lt;const SEED: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.82.0/std/primitive.u64.html\">u64</a>, const MEMBERS_MIN: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.82.0/std/primitive.u64.html\">u64</a>, const MEMBERS_MAX: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.82.0/std/primitive.u64.html\">u64</a>, const OVERLAP_MIN: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.82.0/std/primitive.u64.html\">u64</a>, const OVERLAP_MAX: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.82.0/std/primitive.u64.html\">u64</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot/traits/election/helpers/struct.RandomOverlapQuorumFilterConfig.html\" title=\"struct hotshot::traits::election::helpers::RandomOverlapQuorumFilterConfig\">RandomOverlapQuorumFilterConfig</a>&lt;SEED, MEMBERS_MIN, MEMBERS_MAX, OVERLAP_MIN, OVERLAP_MAX&gt;"],["impl&lt;const SEED: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.82.0/std/primitive.u64.html\">u64</a>, const OVERLAP: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.82.0/std/primitive.u64.html\">u64</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot/traits/election/helpers/struct.StableQuorumFilterConfig.html\" title=\"struct hotshot::traits::election::helpers::StableQuorumFilterConfig\">StableQuorumFilterConfig</a>&lt;SEED, OVERLAP&gt;"]]],["hotshot_example_types",[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot_example_types/block_types/struct.TestMetadata.html\" title=\"struct hotshot_example_types::block_types::TestMetadata\">TestMetadata</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot_example_types/node_types/struct.TestConsecutiveLeaderTypes.html\" title=\"struct hotshot_example_types::node_types::TestConsecutiveLeaderTypes\">TestConsecutiveLeaderTypes</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot_example_types/node_types/struct.TestTypes.html\" title=\"struct hotshot_example_types::node_types::TestTypes\">TestTypes</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot_example_types/node_types/struct.TestTypesRandomizedLeader.html\" title=\"struct hotshot_example_types::node_types::TestTypesRandomizedLeader\">TestTypesRandomizedLeader</a>"],["impl&lt;CONFIG: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> + <a class=\"trait\" href=\"hotshot/traits/election/helpers/trait.QuorumFilterConfig.html\" title=\"trait hotshot::traits::election::helpers::QuorumFilterConfig\">QuorumFilterConfig</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot_example_types/node_types/struct.TestTypesRandomizedCommitteeMembers.html\" title=\"struct hotshot_example_types::node_types::TestTypesRandomizedCommitteeMembers\">TestTypesRandomizedCommitteeMembers</a>&lt;CONFIG&gt;"]]],["hotshot_task_impls",[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"enum\" href=\"hotshot_task_impls/view_sync/enum.ViewSyncPhase.html\" title=\"enum hotshot_task_impls::view_sync::ViewSyncPhase\">ViewSyncPhase</a>"]]],["hotshot_types",[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot_types/data/struct.EpochNumber.html\" title=\"struct hotshot_types::data::EpochNumber\">EpochNumber</a>"],["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot_types/data/struct.ViewNumber.html\" title=\"struct hotshot_types::data::ViewNumber\">ViewNumber</a>"],["impl&lt;F: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> + PrimeField&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot_types/light_client/struct.GenericLightClientState.html\" title=\"struct hotshot_types::light_client::GenericLightClientState\">GenericLightClientState</a>&lt;F&gt;"],["impl&lt;F: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> + PrimeField&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"struct\" href=\"hotshot_types/light_client/struct.GenericStakeTableState.html\" title=\"struct hotshot_types::light_client::GenericStakeTableState\">GenericStakeTableState</a>&lt;F&gt;"]]],["utils",[["impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/cmp/trait.PartialOrd.html\" title=\"trait core::cmp::PartialOrd\">PartialOrd</a> for <a class=\"enum\" href=\"utils/anytrace/enum.Level.html\" title=\"enum utils::anytrace::Level\">Level</a>"]]]]);
    if (window.register_implementors) {
        window.register_implementors(implementors);
    } else {
        window.pending_implementors = implementors;
    }
})()
//{"start":57,"fragment_lengths":[1651,2166,341,1656,276]}