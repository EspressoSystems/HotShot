(function() {
    var implementors = Object.fromEntries([["libp2p_networking",[["impl&lt;T: Transport, Types: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, C: StreamMuxer + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/marker/trait.Unpin.html\" title=\"trait core::marker::Unpin\">Unpin</a>&gt; Transport for <a class=\"struct\" href=\"libp2p_networking/network/transport/struct.StakeTableAuthentication.html\" title=\"struct libp2p_networking::network::transport::StakeTableAuthentication\">StakeTableAuthentication</a>&lt;T, Types, C&gt;<div class=\"where\">where\n    T::Dial: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/future/future/trait.Future.html\" title=\"trait core::future::future::Future\">Future</a>&lt;Output = <a class=\"enum\" href=\"https://doc.rust-lang.org/1.82.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;T::Output, T::Error&gt;&gt; + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + 'static,\n    T::ListenerUpgrade: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + 'static,\n    T::Output: <a class=\"trait\" href=\"libp2p_networking/network/transport/trait.AsOutput.html\" title=\"trait libp2p_networking::network::transport::AsOutput\">AsOutput</a>&lt;C&gt; + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,\n    T::Error: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;&lt;C as StreamMuxer&gt;::Error&gt; + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.82.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt;,\n    C::Substream: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/marker/trait.Unpin.html\" title=\"trait core::marker::Unpin\">Unpin</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.82.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,</div>"]]]]);
    if (window.register_implementors) {
        window.register_implementors(implementors);
    } else {
        window.pending_implementors = implementors;
    }
})()
//{"start":57,"fragment_lengths":[2413]}