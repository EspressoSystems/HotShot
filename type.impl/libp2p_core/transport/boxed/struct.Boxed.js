(function() {var type_impls = {
"libp2p_networking":[["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Debug-for-Boxed%3CO%3E\" class=\"impl\"><a href=\"#impl-Debug-for-Boxed%3CO%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;O&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> for Boxed&lt;O&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.fmt\" class=\"method trait-impl\"><a href=\"#method.fmt\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html#tymethod.fmt\" class=\"fn\">fmt</a>(&amp;self, f: &amp;mut <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/struct.Formatter.html\" title=\"struct core::fmt::Formatter\">Formatter</a>&lt;'_&gt;) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.unit.html\">()</a>, <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/struct.Error.html\" title=\"struct core::fmt::Error\">Error</a>&gt;</h4></section></summary><div class='docblock'>Formats the value using the given formatter. <a href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html#tymethod.fmt\">Read more</a></div></details></div></details>","Debug","libp2p_networking::network::BoxedTransport"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Transport-for-Boxed%3CO%3E\" class=\"impl\"><a href=\"#impl-Transport-for-Boxed%3CO%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;O&gt; Transport for Boxed&lt;O&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle\" open><summary><section id=\"associatedtype.Output\" class=\"associatedtype trait-impl\"><a href=\"#associatedtype.Output\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a class=\"associatedtype\">Output</a> = O</h4></section></summary><div class='docblock'>The result of a connection setup process, including protocol upgrades. <a>Read more</a></div></details><details class=\"toggle\" open><summary><section id=\"associatedtype.Error\" class=\"associatedtype trait-impl\"><a href=\"#associatedtype.Error\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a class=\"associatedtype\">Error</a> = <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a></h4></section></summary><div class='docblock'>An error that occurred during connection setup.</div></details><details class=\"toggle\" open><summary><section id=\"associatedtype.ListenerUpgrade\" class=\"associatedtype trait-impl\"><a href=\"#associatedtype.ListenerUpgrade\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a class=\"associatedtype\">ListenerUpgrade</a> = <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/pin/struct.Pin.html\" title=\"struct core::pin::Pin\">Pin</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/alloc/boxed/struct.Box.html\" title=\"struct alloc::boxed::Box\">Box</a>&lt;dyn <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/future/future/trait.Future.html\" title=\"trait core::future::future::Future\">Future</a>&lt;Output = <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;O, <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt;&gt; + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>&gt;&gt;</h4></section></summary><div class='docblock'>A pending <a href=\"Transport::Output\"><code>Output</code></a> for an inbound connection,\nobtained from the [<code>Transport</code>] stream. <a>Read more</a></div></details><details class=\"toggle\" open><summary><section id=\"associatedtype.Dial\" class=\"associatedtype trait-impl\"><a href=\"#associatedtype.Dial\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a class=\"associatedtype\">Dial</a> = <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/pin/struct.Pin.html\" title=\"struct core::pin::Pin\">Pin</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/alloc/boxed/struct.Box.html\" title=\"struct alloc::boxed::Box\">Box</a>&lt;dyn <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/future/future/trait.Future.html\" title=\"trait core::future::future::Future\">Future</a>&lt;Output = <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;O, <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt;&gt; + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>&gt;&gt;</h4></section></summary><div class='docblock'>A pending <a href=\"Transport::Output\"><code>Output</code></a> for an outbound connection,\nobtained from <a href=\"Transport::dial\">dialing</a>.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.listen_on\" class=\"method trait-impl\"><a href=\"#method.listen_on\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">listen_on</a>(\n    &amp;mut self,\n    id: ListenerId,\n    addr: <a class=\"struct\" href=\"libp2p_networking/reexport/struct.Multiaddr.html\" title=\"struct libp2p_networking::reexport::Multiaddr\">Multiaddr</a>\n) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.unit.html\">()</a>, TransportError&lt;&lt;Boxed&lt;O&gt; as Transport&gt;::Error&gt;&gt;</h4></section></summary><div class='docblock'>Listens on the given <a href=\"libp2p_networking/reexport/struct.Multiaddr.html\" title=\"struct libp2p_networking::reexport::Multiaddr\"><code>Multiaddr</code></a> for inbound connections with a provided [<code>ListenerId</code>].</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.remove_listener\" class=\"method trait-impl\"><a href=\"#method.remove_listener\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">remove_listener</a>(&amp;mut self, id: ListenerId) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>Remove a listener. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.dial\" class=\"method trait-impl\"><a href=\"#method.dial\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">dial</a>(\n    &amp;mut self,\n    addr: <a class=\"struct\" href=\"libp2p_networking/reexport/struct.Multiaddr.html\" title=\"struct libp2p_networking::reexport::Multiaddr\">Multiaddr</a>\n) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;&lt;Boxed&lt;O&gt; as Transport&gt;::Dial, TransportError&lt;&lt;Boxed&lt;O&gt; as Transport&gt;::Error&gt;&gt;</h4></section></summary><div class='docblock'>Dials the given <a href=\"libp2p_networking/reexport/struct.Multiaddr.html\" title=\"struct libp2p_networking::reexport::Multiaddr\"><code>Multiaddr</code></a>, returning a future for a pending outbound connection. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.dial_as_listener\" class=\"method trait-impl\"><a href=\"#method.dial_as_listener\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">dial_as_listener</a>(\n    &amp;mut self,\n    addr: <a class=\"struct\" href=\"libp2p_networking/reexport/struct.Multiaddr.html\" title=\"struct libp2p_networking::reexport::Multiaddr\">Multiaddr</a>\n) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;&lt;Boxed&lt;O&gt; as Transport&gt;::Dial, TransportError&lt;&lt;Boxed&lt;O&gt; as Transport&gt;::Error&gt;&gt;</h4></section></summary><div class='docblock'>As [<code>Transport::dial</code>] but has the local node act as a listener on the outgoing connection. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.address_translation\" class=\"method trait-impl\"><a href=\"#method.address_translation\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">address_translation</a>(\n    &amp;self,\n    server: &amp;<a class=\"struct\" href=\"libp2p_networking/reexport/struct.Multiaddr.html\" title=\"struct libp2p_networking::reexport::Multiaddr\">Multiaddr</a>,\n    observed: &amp;<a class=\"struct\" href=\"libp2p_networking/reexport/struct.Multiaddr.html\" title=\"struct libp2p_networking::reexport::Multiaddr\">Multiaddr</a>\n) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;<a class=\"struct\" href=\"libp2p_networking/reexport/struct.Multiaddr.html\" title=\"struct libp2p_networking::reexport::Multiaddr\">Multiaddr</a>&gt;</h4></section></summary><div class='docblock'>Performs a transport-specific mapping of an address <code>observed</code> by a remote onto a\nlocal <code>listen</code> address to yield an address for the local node that may be reachable\nfor other peers. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.poll\" class=\"method trait-impl\"><a href=\"#method.poll\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">poll</a>(\n    self: <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/pin/struct.Pin.html\" title=\"struct core::pin::Pin\">Pin</a>&lt;&amp;mut Boxed&lt;O&gt;&gt;,\n    cx: &amp;mut <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/task/wake/struct.Context.html\" title=\"struct core::task::wake::Context\">Context</a>&lt;'_&gt;\n) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/task/poll/enum.Poll.html\" title=\"enum core::task::poll::Poll\">Poll</a>&lt;TransportEvent&lt;&lt;Boxed&lt;O&gt; as Transport&gt;::ListenerUpgrade, &lt;Boxed&lt;O&gt; as Transport&gt;::Error&gt;&gt;</h4></section></summary><div class='docblock'>Poll for [<code>TransportEvent</code>]s. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.map\" class=\"method trait-impl\"><a href=\"#method.map\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">map</a>&lt;F, O&gt;(self, f: F) -&gt; Map&lt;Self, F&gt;<div class=\"where\">where\n    Self: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>,\n    F: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/ops/function/trait.FnOnce.html\" title=\"trait core::ops::function::FnOnce\">FnOnce</a>(Self::Output, ConnectedPoint) -&gt; O,</div></h4></section></summary><div class='docblock'>Applies a function on the connections created by the transport.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.map_err\" class=\"method trait-impl\"><a href=\"#method.map_err\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">map_err</a>&lt;F, E&gt;(self, f: F) -&gt; MapErr&lt;Self, F&gt;<div class=\"where\">where\n    Self: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>,\n    F: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/ops/function/trait.FnOnce.html\" title=\"trait core::ops::function::FnOnce\">FnOnce</a>(Self::Error) -&gt; E,</div></h4></section></summary><div class='docblock'>Applies a function on the errors generated by the futures of the transport.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.or_transport\" class=\"method trait-impl\"><a href=\"#method.or_transport\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">or_transport</a>&lt;U&gt;(self, other: U) -&gt; OrTransport&lt;Self, U&gt;<div class=\"where\">where\n    Self: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>,\n    U: Transport,\n    &lt;U as Transport&gt;::Error: 'static,</div></h4></section></summary><div class='docblock'>Adds a fallback transport that is used when encountering errors\nwhile establishing inbound or outbound connections. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.and_then\" class=\"method trait-impl\"><a href=\"#method.and_then\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">and_then</a>&lt;C, F, O&gt;(self, f: C) -&gt; AndThen&lt;Self, C&gt;<div class=\"where\">where\n    Self: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>,\n    C: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/ops/function/trait.FnOnce.html\" title=\"trait core::ops::function::FnOnce\">FnOnce</a>(Self::Output, ConnectedPoint) -&gt; F,\n    F: TryFuture&lt;Ok = O&gt;,\n    &lt;F as TryFuture&gt;::Error: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html\" title=\"trait core::error::Error\">Error</a> + 'static,</div></h4></section></summary><div class='docblock'>Applies a function producing an asynchronous result to every connection\ncreated by this transport. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.upgrade\" class=\"method trait-impl\"><a href=\"#method.upgrade\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">upgrade</a>(self, version: Version) -&gt; Builder&lt;Self&gt;<div class=\"where\">where\n    Self: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>,\n    Self::Error: 'static,</div></h4></section></summary><div class='docblock'>Begins a series of protocol upgrades via an [<code>upgrade::Builder</code>].</div></details></div></details>","Transport","libp2p_networking::network::BoxedTransport"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Stream-for-Boxed%3CO%3E\" class=\"impl\"><a href=\"#impl-Stream-for-Boxed%3CO%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;O&gt; Stream for Boxed&lt;O&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle\" open><summary><section id=\"associatedtype.Item\" class=\"associatedtype trait-impl\"><a href=\"#associatedtype.Item\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a class=\"associatedtype\">Item</a> = TransportEvent&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/pin/struct.Pin.html\" title=\"struct core::pin::Pin\">Pin</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/alloc/boxed/struct.Box.html\" title=\"struct alloc::boxed::Box\">Box</a>&lt;dyn <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/future/future/trait.Future.html\" title=\"trait core::future::future::Future\">Future</a>&lt;Output = <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;O, <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt;&gt; + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>&gt;&gt;, <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt;</h4></section></summary><div class='docblock'>Values yielded by the stream.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.poll_next\" class=\"method trait-impl\"><a href=\"#method.poll_next\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">poll_next</a>(\n    self: <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/pin/struct.Pin.html\" title=\"struct core::pin::Pin\">Pin</a>&lt;&amp;mut Boxed&lt;O&gt;&gt;,\n    cx: &amp;mut <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/task/wake/struct.Context.html\" title=\"struct core::task::wake::Context\">Context</a>&lt;'_&gt;\n) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/task/poll/enum.Poll.html\" title=\"enum core::task::poll::Poll\">Poll</a>&lt;<a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;&lt;Boxed&lt;O&gt; as Stream&gt;::Item&gt;&gt;</h4></section></summary><div class='docblock'>Attempt to pull out the next value of this stream, registering the\ncurrent task for wakeup if the value is not yet available, and returning\n<code>None</code> if the stream is exhausted. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.size_hint\" class=\"method trait-impl\"><a href=\"#method.size_hint\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">size_hint</a>(&amp;self) -&gt; (<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.usize.html\">usize</a>, <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.usize.html\">usize</a>&gt;)</h4></section></summary><div class='docblock'>Returns the bounds on the remaining length of the stream. <a>Read more</a></div></details></div></details>","Stream","libp2p_networking::network::BoxedTransport"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-FusedStream-for-Boxed%3CO%3E\" class=\"impl\"><a href=\"#impl-FusedStream-for-Boxed%3CO%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;O&gt; FusedStream for Boxed&lt;O&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.is_terminated\" class=\"method trait-impl\"><a href=\"#method.is_terminated\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">is_terminated</a>(&amp;self) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>Returns <code>true</code> if the stream should no longer be polled.</div></details></div></details>","FusedStream","libp2p_networking::network::BoxedTransport"]]
};if (window.register_type_impls) {window.register_type_impls(type_impls);} else {window.pending_type_impls = type_impls;}})()