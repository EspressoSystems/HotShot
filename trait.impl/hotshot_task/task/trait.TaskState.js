(function() {
    var implementors = Object.fromEntries([["hotshot_task_impls",[["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/rewind/struct.RewindTaskState.html\" title=\"struct hotshot_task_impls::rewind::RewindTaskState\">RewindTaskState</a>&lt;TYPES&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, I: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeImplementation.html\" title=\"trait hotshot_types::traits::node_implementation::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/request/struct.NetworkRequestState.html\" title=\"struct hotshot_task_impls::request::NetworkRequestState\">NetworkRequestState</a>&lt;TYPES, I&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, I: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeImplementation.html\" title=\"trait hotshot_types::traits::node_implementation::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/vid/struct.VidTaskState.html\" title=\"struct hotshot_task_impls::vid::VidTaskState\">VidTaskState</a>&lt;TYPES, I&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, I: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeImplementation.html\" title=\"trait hotshot_types::traits::node_implementation::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/consensus/struct.ConsensusTaskState.html\" title=\"struct hotshot_task_impls::consensus::ConsensusTaskState\">ConsensusTaskState</a>&lt;TYPES, I, V&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, I: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeImplementation.html\" title=\"trait hotshot_types::traits::node_implementation::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/da/struct.DaTaskState.html\" title=\"struct hotshot_task_impls::da::DaTaskState\">DaTaskState</a>&lt;TYPES, I, V&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, I: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeImplementation.html\" title=\"trait hotshot_types::traits::node_implementation::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/quorum_proposal/struct.QuorumProposalTaskState.html\" title=\"struct hotshot_task_impls::quorum_proposal::QuorumProposalTaskState\">QuorumProposalTaskState</a>&lt;TYPES, I, V&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, I: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeImplementation.html\" title=\"trait hotshot_types::traits::node_implementation::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/quorum_proposal_recv/struct.QuorumProposalRecvTaskState.html\" title=\"struct hotshot_task_impls::quorum_proposal_recv::QuorumProposalRecvTaskState\">QuorumProposalRecvTaskState</a>&lt;TYPES, I, V&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, I: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeImplementation.html\" title=\"trait hotshot_types::traits::node_implementation::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/quorum_vote/struct.QuorumVoteTaskState.html\" title=\"struct hotshot_task_impls::quorum_vote::QuorumVoteTaskState\">QuorumVoteTaskState</a>&lt;TYPES, I, V&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, I: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeImplementation.html\" title=\"trait hotshot_types::traits::node_implementation::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/transactions/struct.TransactionTaskState.html\" title=\"struct hotshot_task_impls::transactions::TransactionTaskState\">TransactionTaskState</a>&lt;TYPES, I, V&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/upgrade/struct.UpgradeTaskState.html\" title=\"struct hotshot_task_impls::upgrade::UpgradeTaskState\">UpgradeTaskState</a>&lt;TYPES, V&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/view_sync/struct.ViewSyncReplicaTaskState.html\" title=\"struct hotshot_task_impls::view_sync::ViewSyncReplicaTaskState\">ViewSyncReplicaTaskState</a>&lt;TYPES, V&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/view_sync/struct.ViewSyncTaskState.html\" title=\"struct hotshot_task_impls::view_sync::ViewSyncTaskState\">ViewSyncTaskState</a>&lt;TYPES, V&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>, NET: <a class=\"trait\" href=\"hotshot_types/traits/network/trait.ConnectedNetwork.html\" title=\"trait hotshot_types::traits::network::ConnectedNetwork\">ConnectedNetwork</a>&lt;TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a>&gt;, S: <a class=\"trait\" href=\"hotshot_types/traits/storage/trait.Storage.html\" title=\"trait hotshot_types::traits::storage::Storage\">Storage</a>&lt;TYPES&gt; + 'static&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/network/struct.NetworkEventTaskState.html\" title=\"struct hotshot_task_impls::network::NetworkEventTaskState\">NetworkEventTaskState</a>&lt;TYPES, V, NET, S&gt;"],["impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, V: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.Versions.html\" title=\"trait hotshot_types::traits::node_implementation::Versions\">Versions</a>, NET: <a class=\"trait\" href=\"hotshot_types/traits/network/trait.ConnectedNetwork.html\" title=\"trait hotshot_types::traits::network::ConnectedNetwork\">ConnectedNetwork</a>&lt;TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a>&gt;, S: <a class=\"trait\" href=\"hotshot_types/traits/storage/trait.Storage.html\" title=\"trait hotshot_types::traits::storage::Storage\">Storage</a>&lt;TYPES&gt; + 'static&gt; <a class=\"trait\" href=\"hotshot_task/task/trait.TaskState.html\" title=\"trait hotshot_task::task::TaskState\">TaskState</a> for <a class=\"struct\" href=\"hotshot_task_impls/network/test/struct.NetworkEventTaskStateModifier.html\" title=\"struct hotshot_task_impls::network::test::NetworkEventTaskStateModifier\">NetworkEventTaskStateModifier</a>&lt;TYPES, V, NET, S&gt;"]]]]);
    if (window.register_implementors) {
        window.register_implementors(implementors);
    } else {
        window.pending_implementors = implementors;
    }
})()
//{"start":57,"fragment_lengths":[12242]}