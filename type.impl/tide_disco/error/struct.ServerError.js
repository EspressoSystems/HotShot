(function() {var type_impls = {
"hotshot_web_server":[["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Error-for-ServerError\" class=\"impl\"><a href=\"#impl-Error-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html\" title=\"trait core::error::Error\">Error</a> for ServerError<div class=\"where\">where\n    ServerError: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Display.html\" title=\"trait core::fmt::Display\">Display</a>,</div></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.description\" class=\"method trait-impl\"><a href=\"#method.description\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html#method.description\" class=\"fn\">description</a>(&amp;self) -&gt; &amp;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.str.html\">str</a></h4></section></summary><span class=\"item-info\"><div class=\"stab deprecated\"><span class=\"emoji\">👎</span><span>Deprecated since 1.42.0: use the Display impl or to_string()</span></div></span><div class='docblock'> <a href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html#method.description\">Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.cause\" class=\"method trait-impl\"><a href=\"#method.cause\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html#method.cause\" class=\"fn\">cause</a>(&amp;self) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;&amp;dyn <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html\" title=\"trait core::error::Error\">Error</a>&gt;</h4></section></summary><span class=\"item-info\"><div class=\"stab deprecated\"><span class=\"emoji\">👎</span><span>Deprecated since 1.33.0: replaced by Error::source, which can support downcasting</span></div></span></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.source\" class=\"method trait-impl\"><a href=\"#method.source\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html#method.source\" class=\"fn\">source</a>(&amp;self) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;&amp;(dyn <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html\" title=\"trait core::error::Error\">Error</a> + 'static)&gt;</h4></section></summary><div class='docblock'>The lower-level source of this error, if any. <a href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html#method.source\">Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.provide\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"https://doc.rust-lang.org/1.76.0/src/core/error.rs.html#184\">source</a><a href=\"#method.provide\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html#method.provide\" class=\"fn\">provide</a>&lt;'a&gt;(&amp;'a self, request: &amp;mut <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/error/struct.Request.html\" title=\"struct core::error::Request\">Request</a>&lt;'a&gt;)</h4></section></summary><span class=\"item-info\"><div class=\"stab unstable\"><span class=\"emoji\">🔬</span><span>This is a nightly-only experimental API. (<code>error_generic_member_access</code>)</span></div></span><div class='docblock'>Provides type based access to context intended for error reports. <a href=\"https://doc.rust-lang.org/1.76.0/core/error/trait.Error.html#method.provide\">Read more</a></div></details></div></details>","Error","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-From%3CSocketError%3CE%3E%3E-for-ServerError\" class=\"impl\"><a href=\"#impl-From%3CSocketError%3CE%3E%3E-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;SocketError&lt;E&gt;&gt; for ServerError<div class=\"where\">where\n    E: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Display.html\" title=\"trait core::fmt::Display\">Display</a>,</div></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.from\" class=\"method trait-impl\"><a href=\"#method.from\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/convert/trait.From.html#tymethod.from\" class=\"fn\">from</a>(source: SocketError&lt;E&gt;) -&gt; ServerError</h4></section></summary><div class='docblock'>Converts to this type from the input type.</div></details></div></details>","From<SocketError<E>>","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-From%3CConfigError%3E-for-ServerError\" class=\"impl\"><a href=\"#impl-From%3CConfigError%3E-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;ConfigError&gt; for ServerError</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.from\" class=\"method trait-impl\"><a href=\"#method.from\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/convert/trait.From.html#tymethod.from\" class=\"fn\">from</a>(source: ConfigError) -&gt; ServerError</h4></section></summary><div class='docblock'>Converts to this type from the input type.</div></details></div></details>","From<ConfigError>","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-From%3CRouteError%3CE%3E%3E-for-ServerError\" class=\"impl\"><a href=\"#impl-From%3CRouteError%3CE%3E%3E-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;RouteError&lt;E&gt;&gt; for ServerError<div class=\"where\">where\n    E: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Display.html\" title=\"trait core::fmt::Display\">Display</a>,</div></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.from\" class=\"method trait-impl\"><a href=\"#method.from\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/convert/trait.From.html#tymethod.from\" class=\"fn\">from</a>(source: RouteError&lt;E&gt;) -&gt; ServerError</h4></section></summary><div class='docblock'>Converts to this type from the input type.</div></details></div></details>","From<RouteError<E>>","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-From%3CRequestError%3E-for-ServerError\" class=\"impl\"><a href=\"#impl-From%3CRequestError%3E-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;RequestError&gt; for ServerError</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.from\" class=\"method trait-impl\"><a href=\"#method.from\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/convert/trait.From.html#tymethod.from\" class=\"fn\">from</a>(source: RequestError) -&gt; ServerError</h4></section></summary><div class='docblock'>Converts to this type from the input type.</div></details></div></details>","From<RequestError>","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-From%3CError%3E-for-ServerError\" class=\"impl\"><a href=\"#impl-From%3CError%3E-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/convert/trait.From.html\" title=\"trait core::convert::From\">From</a>&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>&gt; for ServerError</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.from\" class=\"method trait-impl\"><a href=\"#method.from\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/convert/trait.From.html#tymethod.from\" class=\"fn\">from</a>(source: <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>) -&gt; ServerError</h4></section></summary><div class='docblock'>Converts to this type from the input type.</div></details></div></details>","From<Error>","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Deserialize%3C'de%3E-for-ServerError\" class=\"impl\"><a href=\"#impl-Deserialize%3C'de%3E-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;'de&gt; <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserialize.html\" title=\"trait serde::de::Deserialize\">Deserialize</a>&lt;'de&gt; for ServerError</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.deserialize\" class=\"method trait-impl\"><a href=\"#method.deserialize\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserialize.html#tymethod.deserialize\" class=\"fn\">deserialize</a>&lt;__D&gt;(\n    __deserializer: __D\n) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;ServerError, &lt;__D as <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserializer.html\" title=\"trait serde::de::Deserializer\">Deserializer</a>&lt;'de&gt;&gt;::<a class=\"associatedtype\" href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserializer.html#associatedtype.Error\" title=\"type serde::de::Deserializer::Error\">Error</a>&gt;<div class=\"where\">where\n    __D: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserializer.html\" title=\"trait serde::de::Deserializer\">Deserializer</a>&lt;'de&gt;,</div></h4></section></summary><div class='docblock'>Deserialize this value from the given Serde deserializer. <a href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserialize.html#tymethod.deserialize\">Read more</a></div></details></div></details>","Deserialize<'de>","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Debug-for-ServerError\" class=\"impl\"><a href=\"#impl-Debug-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> for ServerError</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.fmt\" class=\"method trait-impl\"><a href=\"#method.fmt\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html#tymethod.fmt\" class=\"fn\">fmt</a>(&amp;self, f: &amp;mut <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/struct.Formatter.html\" title=\"struct core::fmt::Formatter\">Formatter</a>&lt;'_&gt;) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.unit.html\">()</a>, <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/struct.Error.html\" title=\"struct core::fmt::Error\">Error</a>&gt;</h4></section></summary><div class='docblock'>Formats the value using the given formatter. <a href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html#tymethod.fmt\">Read more</a></div></details></div></details>","Debug","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Error-for-ServerError\" class=\"impl\"><a href=\"#impl-Error-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl Error for ServerError</h3></section></summary><div class=\"impl-items\"><section id=\"method.catch_all\" class=\"method trait-impl\"><a href=\"#method.catch_all\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">catch_all</a>(status: StatusCode, message: <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/alloc/string/struct.String.html\" title=\"struct alloc::string::String\">String</a>) -&gt; ServerError</h4></section><section id=\"method.status\" class=\"method trait-impl\"><a href=\"#method.status\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">status</a>(&amp;self) -&gt; StatusCode</h4></section><section id=\"method.from_io_error\" class=\"method trait-impl\"><a href=\"#method.from_io_error\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">from_io_error</a>(source: <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/std/io/error/struct.Error.html\" title=\"struct std::io::error::Error\">Error</a>) -&gt; Self</h4></section><section id=\"method.from_config_error\" class=\"method trait-impl\"><a href=\"#method.from_config_error\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">from_config_error</a>(source: ConfigError) -&gt; Self</h4></section><section id=\"method.from_route_error\" class=\"method trait-impl\"><a href=\"#method.from_route_error\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">from_route_error</a>&lt;E&gt;(source: RouteError&lt;E&gt;) -&gt; Self<div class=\"where\">where\n    E: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Display.html\" title=\"trait core::fmt::Display\">Display</a>,</div></h4></section><section id=\"method.from_request_error\" class=\"method trait-impl\"><a href=\"#method.from_request_error\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">from_request_error</a>(source: RequestError) -&gt; Self</h4></section><section id=\"method.from_socket_error\" class=\"method trait-impl\"><a href=\"#method.from_socket_error\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">from_socket_error</a>&lt;E&gt;(source: SocketError&lt;E&gt;) -&gt; Self<div class=\"where\">where\n    E: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Display.html\" title=\"trait core::fmt::Display\">Display</a>,</div></h4></section><section id=\"method.into_tide_error\" class=\"method trait-impl\"><a href=\"#method.into_tide_error\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">into_tide_error</a>(self) -&gt; Error</h4></section><section id=\"method.from_server_error\" class=\"method trait-impl\"><a href=\"#method.from_server_error\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">from_server_error</a>(source: Error) -&gt; Self</h4></section></div></details>","Error","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Serialize-for-ServerError\" class=\"impl\"><a href=\"#impl-Serialize-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serialize.html\" title=\"trait serde::ser::Serialize\">Serialize</a> for ServerError</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.serialize\" class=\"method trait-impl\"><a href=\"#method.serialize\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serialize.html#tymethod.serialize\" class=\"fn\">serialize</a>&lt;__S&gt;(\n    &amp;self,\n    __serializer: __S\n) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;&lt;__S as <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serializer.html\" title=\"trait serde::ser::Serializer\">Serializer</a>&gt;::<a class=\"associatedtype\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serializer.html#associatedtype.Ok\" title=\"type serde::ser::Serializer::Ok\">Ok</a>, &lt;__S as <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serializer.html\" title=\"trait serde::ser::Serializer\">Serializer</a>&gt;::<a class=\"associatedtype\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serializer.html#associatedtype.Error\" title=\"type serde::ser::Serializer::Error\">Error</a>&gt;<div class=\"where\">where\n    __S: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serializer.html\" title=\"trait serde::ser::Serializer\">Serializer</a>,</div></h4></section></summary><div class='docblock'>Serialize this value into the given Serde serializer. <a href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serialize.html#tymethod.serialize\">Read more</a></div></details></div></details>","Serialize","hotshot_web_server::Error"],["<section id=\"impl-StructuralPartialEq-for-ServerError\" class=\"impl\"><a href=\"#impl-StructuralPartialEq-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.StructuralPartialEq.html\" title=\"trait core::marker::StructuralPartialEq\">StructuralPartialEq</a> for ServerError</h3></section>","StructuralPartialEq","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-PartialEq-for-ServerError\" class=\"impl\"><a href=\"#impl-PartialEq-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a> for ServerError</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.eq\" class=\"method trait-impl\"><a href=\"#method.eq\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.PartialEq.html#tymethod.eq\" class=\"fn\">eq</a>(&amp;self, other: &amp;ServerError) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>This method tests for <code>self</code> and <code>other</code> values to be equal, and is used\nby <code>==</code>.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.ne\" class=\"method trait-impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/core/cmp.rs.html#242\">source</a></span><a href=\"#method.ne\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.PartialEq.html#method.ne\" class=\"fn\">ne</a>(&amp;self, other: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.reference.html\">&amp;Rhs</a>) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>This method tests for <code>!=</code>. The default implementation is almost always\nsufficient, and should not be overridden without very good reason.</div></details></div></details>","PartialEq","hotshot_web_server::Error"],["<section id=\"impl-Eq-for-ServerError\" class=\"impl\"><a href=\"#impl-Eq-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.Eq.html\" title=\"trait core::cmp::Eq\">Eq</a> for ServerError</h3></section>","Eq","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-ErrorCompat-for-ServerError\" class=\"impl\"><a href=\"#impl-ErrorCompat-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl ErrorCompat for ServerError</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.backtrace\" class=\"method trait-impl\"><a href=\"#method.backtrace\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">backtrace</a>(&amp;self) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/option/enum.Option.html\" title=\"enum core::option::Option\">Option</a>&lt;&amp;Backtrace&gt;</h4></section></summary><div class='docblock'>Returns a <a href=\"Backtrace\"><code>Backtrace</code></a> that may be printed.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.iter_chain\" class=\"method trait-impl\"><a href=\"#method.iter_chain\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">iter_chain</a>(&amp;self) -&gt; ChainCompat&lt;'_&gt;<div class=\"where\">where\n    Self: AsErrorSource,</div></h4></section></summary><div class='docblock'>Returns an iterator for traversing the chain of errors,\nstarting with the current error\nand continuing with recursive calls to <code>Error::source</code>. <a>Read more</a></div></details></div></details>","ErrorCompat","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Clone-for-ServerError\" class=\"impl\"><a href=\"#impl-Clone-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> for ServerError</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.clone\" class=\"method trait-impl\"><a href=\"#method.clone\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#tymethod.clone\" class=\"fn\">clone</a>(&amp;self) -&gt; ServerError</h4></section></summary><div class='docblock'>Returns a copy of the value. <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#tymethod.clone\">Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.clone_from\" class=\"method trait-impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/core/clone.rs.html#169\">source</a></span><a href=\"#method.clone_from\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#method.clone_from\" class=\"fn\">clone_from</a>(&amp;mut self, source: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.reference.html\">&amp;Self</a>)</h4></section></summary><div class='docblock'>Performs copy-assignment from <code>source</code>. <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#method.clone_from\">Read more</a></div></details></div></details>","Clone","hotshot_web_server::Error"],["<section id=\"impl-StructuralEq-for-ServerError\" class=\"impl\"><a href=\"#impl-StructuralEq-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.StructuralEq.html\" title=\"trait core::marker::StructuralEq\">StructuralEq</a> for ServerError</h3></section>","StructuralEq","hotshot_web_server::Error"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Display-for-ServerError\" class=\"impl\"><a href=\"#impl-Display-for-ServerError\" class=\"anchor\">§</a><h3 class=\"code-header\">impl <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Display.html\" title=\"trait core::fmt::Display\">Display</a> for ServerError</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.fmt\" class=\"method trait-impl\"><a href=\"#method.fmt\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Display.html#tymethod.fmt\" class=\"fn\">fmt</a>(\n    &amp;self,\n    __snafu_display_formatter: &amp;mut <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/struct.Formatter.html\" title=\"struct core::fmt::Formatter\">Formatter</a>&lt;'_&gt;\n) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.unit.html\">()</a>, <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/struct.Error.html\" title=\"struct core::fmt::Error\">Error</a>&gt;</h4></section></summary><div class='docblock'>Formats the value using the given formatter. <a href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Display.html#tymethod.fmt\">Read more</a></div></details></div></details>","Display","hotshot_web_server::Error"]]
};if (window.register_type_impls) {window.register_type_impls(type_impls);} else {window.pending_type_impls = type_impls;}})()