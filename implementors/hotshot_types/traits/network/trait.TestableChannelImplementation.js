(function() {var implementors = {
"hotshot":[["impl&lt;TYPES: NodeType, I: <a class=\"trait\" href=\"hotshot/traits/trait.NodeImplementation.html\" title=\"trait hotshot::traits::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, PROPOSAL: ProposalType&lt;NodeType = TYPES&gt;, VOTE: <a class=\"trait\" href=\"hotshot/types/trait.VoteType.html\" title=\"trait hotshot::types::VoteType\">VoteType</a>&lt;TYPES&gt;, MEMBERSHIP: Membership&lt;TYPES&gt;&gt; TestableChannelImplementation&lt;TYPES, <a class=\"struct\" href=\"hotshot/types/struct.Message.html\" title=\"struct hotshot::types::Message\">Message</a>&lt;TYPES, I&gt;, PROPOSAL, VOTE, MEMBERSHIP, <a class=\"struct\" href=\"hotshot/traits/networking/web_server_network/struct.WebServerNetwork.html\" title=\"struct hotshot::traits::networking::web_server_network::WebServerNetwork\">WebServerNetwork</a>&lt;<a class=\"struct\" href=\"hotshot/types/struct.Message.html\" title=\"struct hotshot::types::Message\">Message</a>&lt;TYPES, I&gt;, &lt;TYPES as NodeType&gt;::SignatureKey, &lt;TYPES as NodeType&gt;::ElectionConfigType, TYPES, PROPOSAL, VOTE&gt;&gt; for <a class=\"struct\" href=\"hotshot/traits/networking/web_server_network/struct.WebCommChannel.html\" title=\"struct hotshot::traits::networking::web_server_network::WebCommChannel\">WebCommChannel</a>&lt;TYPES::ConsensusType, TYPES, I, PROPOSAL, VOTE, MEMBERSHIP&gt;<span class=\"where fmt-newline\">where\n    TYPES::SignatureKey: TestableSignatureKey,</span>"],["impl&lt;TYPES: NodeType, I: <a class=\"trait\" href=\"hotshot/traits/trait.NodeImplementation.html\" title=\"trait hotshot::traits::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, PROPOSAL: ProposalType&lt;NodeType = TYPES&gt;, VOTE: <a class=\"trait\" href=\"hotshot/types/trait.VoteType.html\" title=\"trait hotshot::types::VoteType\">VoteType</a>&lt;TYPES&gt;, MEMBERSHIP: Membership&lt;TYPES&gt;&gt; TestableChannelImplementation&lt;TYPES, <a class=\"struct\" href=\"hotshot/types/struct.Message.html\" title=\"struct hotshot::types::Message\">Message</a>&lt;TYPES, I&gt;, PROPOSAL, VOTE, MEMBERSHIP, <a class=\"struct\" href=\"hotshot/traits/networking/centralized_server_network/struct.CentralizedServerNetwork.html\" title=\"struct hotshot::traits::networking::centralized_server_network::CentralizedServerNetwork\">CentralizedServerNetwork</a>&lt;&lt;TYPES as NodeType&gt;::SignatureKey, &lt;TYPES as NodeType&gt;::ElectionConfigType&gt;&gt; for <a class=\"struct\" href=\"hotshot/traits/networking/centralized_server_network/struct.CentralizedCommChannel.html\" title=\"struct hotshot::traits::networking::centralized_server_network::CentralizedCommChannel\">CentralizedCommChannel</a>&lt;TYPES, I, PROPOSAL, VOTE, MEMBERSHIP&gt;<span class=\"where fmt-newline\">where\n    TYPES::SignatureKey: TestableSignatureKey,</span>"],["impl&lt;TYPES: NodeType, I: <a class=\"trait\" href=\"hotshot/traits/trait.NodeImplementation.html\" title=\"trait hotshot::traits::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, PROPOSAL: ProposalType&lt;NodeType = TYPES&gt;, VOTE: <a class=\"trait\" href=\"hotshot/types/trait.VoteType.html\" title=\"trait hotshot::types::VoteType\">VoteType</a>&lt;TYPES&gt;, MEMBERSHIP: Membership&lt;TYPES&gt;&gt; TestableChannelImplementation&lt;TYPES, <a class=\"struct\" href=\"hotshot/types/struct.Message.html\" title=\"struct hotshot::types::Message\">Message</a>&lt;TYPES, I&gt;, PROPOSAL, VOTE, MEMBERSHIP, <a class=\"struct\" href=\"hotshot/traits/networking/memory_network/struct.MemoryNetwork.html\" title=\"struct hotshot::traits::networking::memory_network::MemoryNetwork\">MemoryNetwork</a>&lt;<a class=\"struct\" href=\"hotshot/types/struct.Message.html\" title=\"struct hotshot::types::Message\">Message</a>&lt;TYPES, I&gt;, &lt;TYPES as NodeType&gt;::SignatureKey&gt;&gt; for <a class=\"struct\" href=\"hotshot/traits/networking/memory_network/struct.MemoryCommChannel.html\" title=\"struct hotshot::traits::networking::memory_network::MemoryCommChannel\">MemoryCommChannel</a>&lt;TYPES, I, PROPOSAL, VOTE, MEMBERSHIP&gt;<span class=\"where fmt-newline\">where\n    TYPES::SignatureKey: TestableSignatureKey,</span>"],["impl&lt;TYPES: NodeType, I: <a class=\"trait\" href=\"hotshot/traits/trait.NodeImplementation.html\" title=\"trait hotshot::traits::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, PROPOSAL: ProposalType&lt;NodeType = TYPES&gt;, VOTE: <a class=\"trait\" href=\"hotshot/types/trait.VoteType.html\" title=\"trait hotshot::types::VoteType\">VoteType</a>&lt;TYPES&gt;, MEMBERSHIP: Membership&lt;TYPES&gt;&gt; TestableChannelImplementation&lt;TYPES, <a class=\"struct\" href=\"hotshot/types/struct.Message.html\" title=\"struct hotshot::types::Message\">Message</a>&lt;TYPES, I&gt;, PROPOSAL, VOTE, MEMBERSHIP, <a class=\"struct\" href=\"hotshot/traits/networking/libp2p_network/struct.Libp2pNetwork.html\" title=\"struct hotshot::traits::networking::libp2p_network::Libp2pNetwork\">Libp2pNetwork</a>&lt;<a class=\"struct\" href=\"hotshot/types/struct.Message.html\" title=\"struct hotshot::types::Message\">Message</a>&lt;TYPES, I&gt;, &lt;TYPES as NodeType&gt;::SignatureKey&gt;&gt; for <a class=\"struct\" href=\"hotshot/traits/networking/libp2p_network/struct.Libp2pCommChannel.html\" title=\"struct hotshot::traits::networking::libp2p_network::Libp2pCommChannel\">Libp2pCommChannel</a>&lt;TYPES, I, PROPOSAL, VOTE, MEMBERSHIP&gt;<span class=\"where fmt-newline\">where\n    TYPES::SignatureKey: TestableSignatureKey,</span>"],["impl&lt;TYPES: NodeType, I: <a class=\"trait\" href=\"hotshot/traits/trait.NodeImplementation.html\" title=\"trait hotshot::traits::NodeImplementation\">NodeImplementation</a>&lt;TYPES&gt;, PROPOSAL: ProposalType&lt;NodeType = TYPES&gt;, VOTE: <a class=\"trait\" href=\"hotshot/types/trait.VoteType.html\" title=\"trait hotshot::types::VoteType\">VoteType</a>&lt;TYPES&gt;, MEMBERSHIP: Membership&lt;TYPES&gt;&gt; TestableChannelImplementation&lt;TYPES, <a class=\"struct\" href=\"hotshot/types/struct.Message.html\" title=\"struct hotshot::types::Message\">Message</a>&lt;TYPES, I&gt;, PROPOSAL, VOTE, MEMBERSHIP, <a class=\"struct\" href=\"hotshot/traits/networking/web_sever_libp2p_fallback/struct.CombinedNetworks.html\" title=\"struct hotshot::traits::networking::web_sever_libp2p_fallback::CombinedNetworks\">CombinedNetworks</a>&lt;TYPES, I, PROPOSAL, VOTE, MEMBERSHIP&gt;&gt; for <a class=\"struct\" href=\"hotshot/traits/networking/web_sever_libp2p_fallback/struct.WebServerWithFallbackCommChannel.html\" title=\"struct hotshot::traits::networking::web_sever_libp2p_fallback::WebServerWithFallbackCommChannel\">WebServerWithFallbackCommChannel</a>&lt;TYPES, I, PROPOSAL, VOTE, MEMBERSHIP&gt;<span class=\"where fmt-newline\">where\n    TYPES::SignatureKey: TestableSignatureKey,</span>"]]
};if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()