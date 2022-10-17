(function() {var implementors = {};
implementors["hotshot"] = [{"text":"impl&lt;M, P, E&gt; <a class=\"trait\" href=\"hotshot/traits/trait.NetworkingImplementation.html\" title=\"trait hotshot::traits::NetworkingImplementation\">NetworkingImplementation</a>&lt;M, P&gt; for <a class=\"struct\" href=\"hotshot/traits/implementations/struct.CentralizedServerNetwork.html\" title=\"struct hotshot::traits::implementations::CentralizedServerNetwork\">CentralizedServerNetwork</a>&lt;P, E&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;M: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.145/serde/ser/trait.Serialize.html\" title=\"trait serde::ser::Serialize\">Serialize</a> + <a class=\"trait\" href=\"https://docs.rs/serde/1.0.145/serde/de/trait.DeserializeOwned.html\" title=\"trait serde::de::DeserializeOwned\">DeserializeOwned</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + 'static,<br>&nbsp;&nbsp;&nbsp;&nbsp;P: <a class=\"trait\" href=\"hotshot/types/trait.SignatureKey.html\" title=\"trait hotshot::types::SignatureKey\">SignatureKey</a> + 'static,<br>&nbsp;&nbsp;&nbsp;&nbsp;E: ElectionConfig + 'static,&nbsp;</span>","synthetic":false,"types":["hotshot::traits::networking::centralized_server_network::CentralizedServerNetwork"]},{"text":"impl&lt;M:&nbsp;<a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + <a class=\"trait\" href=\"https://docs.rs/serde/1.0.145/serde/ser/trait.Serialize.html\" title=\"trait serde::ser::Serialize\">Serialize</a> + <a class=\"trait\" href=\"https://docs.rs/serde/1.0.145/serde/de/trait.DeserializeOwned.html\" title=\"trait serde::de::DeserializeOwned\">DeserializeOwned</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> + 'static, P:&nbsp;<a class=\"trait\" href=\"hotshot/types/trait.SignatureKey.html\" title=\"trait hotshot::types::SignatureKey\">SignatureKey</a> + 'static&gt; <a class=\"trait\" href=\"hotshot/traits/trait.NetworkingImplementation.html\" title=\"trait hotshot::traits::NetworkingImplementation\">NetworkingImplementation</a>&lt;M, P&gt; for <a class=\"struct\" href=\"hotshot/traits/implementations/struct.Libp2pNetwork.html\" title=\"struct hotshot::traits::implementations::Libp2pNetwork\">Libp2pNetwork</a>&lt;M, P&gt;","synthetic":false,"types":["hotshot::traits::networking::libp2p_network::Libp2pNetwork"]},{"text":"impl&lt;T:&nbsp;<a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + <a class=\"trait\" href=\"https://docs.rs/serde/1.0.145/serde/ser/trait.Serialize.html\" title=\"trait serde::ser::Serialize\">Serialize</a> + <a class=\"trait\" href=\"https://docs.rs/serde/1.0.145/serde/de/trait.DeserializeOwned.html\" title=\"trait serde::de::DeserializeOwned\">DeserializeOwned</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> + 'static, P:&nbsp;<a class=\"trait\" href=\"hotshot/types/trait.SignatureKey.html\" title=\"trait hotshot::types::SignatureKey\">SignatureKey</a> + 'static&gt; <a class=\"trait\" href=\"hotshot/traits/trait.NetworkingImplementation.html\" title=\"trait hotshot::traits::NetworkingImplementation\">NetworkingImplementation</a>&lt;T, P&gt; for <a class=\"struct\" href=\"hotshot/traits/implementations/struct.MemoryNetwork.html\" title=\"struct hotshot::traits::implementations::MemoryNetwork\">MemoryNetwork</a>&lt;T, P&gt;","synthetic":false,"types":["hotshot::traits::networking::memory_network::MemoryNetwork"]},{"text":"impl&lt;T:&nbsp;<a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + <a class=\"trait\" href=\"https://docs.rs/serde/1.0.145/serde/ser/trait.Serialize.html\" title=\"trait serde::ser::Serialize\">Serialize</a> + <a class=\"trait\" href=\"https://docs.rs/serde/1.0.145/serde/de/trait.DeserializeOwned.html\" title=\"trait serde::de::DeserializeOwned\">DeserializeOwned</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.64.0/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> + 'static, P:&nbsp;<a class=\"trait\" href=\"hotshot/types/trait.SignatureKey.html\" title=\"trait hotshot::types::SignatureKey\">SignatureKey</a> + 'static&gt; <a class=\"trait\" href=\"hotshot/traits/trait.NetworkingImplementation.html\" title=\"trait hotshot::traits::NetworkingImplementation\">NetworkingImplementation</a>&lt;T, P&gt; for <a class=\"struct\" href=\"hotshot/traits/implementations/struct.WNetwork.html\" title=\"struct hotshot::traits::implementations::WNetwork\">WNetwork</a>&lt;T, P&gt;","synthetic":false,"types":["hotshot::traits::networking::w_network::WNetwork"]}];
if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()