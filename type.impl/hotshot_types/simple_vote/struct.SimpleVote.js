(function() {var type_impls = {
"hotshot_types":[["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#142-161\">source</a><a href=\"#impl-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA: <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a> + 'static&gt; <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.create_signed_vote\" class=\"method\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#146-160\">source</a><h4 class=\"code-header\">pub fn <a href=\"hotshot_types/simple_vote/struct.SimpleVote.html#tymethod.create_signed_vote\" class=\"fn\">create_signed_vote</a>(\n    data: DATA,\n    view: TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.Time\" title=\"type hotshot_types::traits::node_implementation::NodeType::Time\">Time</a>,\n    pub_key: &amp;TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a>,\n    private_key: &amp;&lt;TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a> as <a class=\"trait\" href=\"hotshot_types/traits/signature_key/trait.SignatureKey.html\" title=\"trait hotshot_types::traits::signature_key::SignatureKey\">SignatureKey</a>&gt;::<a class=\"associatedtype\" href=\"hotshot_types/traits/signature_key/trait.SignatureKey.html#associatedtype.PrivateKey\" title=\"type hotshot_types::traits::signature_key::SignatureKey::PrivateKey\">PrivateKey</a>\n) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;Self, &lt;TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a> as <a class=\"trait\" href=\"hotshot_types/traits/signature_key/trait.SignatureKey.html\" title=\"trait hotshot_types::traits::signature_key::SignatureKey\">SignatureKey</a>&gt;::<a class=\"associatedtype\" href=\"hotshot_types/traits/signature_key/trait.SignatureKey.html#associatedtype.SignError\" title=\"type hotshot_types::traits::signature_key::SignatureKey::SignError\">SignError</a>&gt;</h4></section></summary><div class=\"docblock\"><p>Creates and signs a simple vote</p>\n<h5 id=\"errors\"><a href=\"#errors\">Errors</a></h5>\n<p>If we are unable to sign the data</p>\n</div></details></div></details>",0,"hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Vote%3CTYPES%3E-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#122-140\">source</a><a href=\"#impl-Vote%3CTYPES%3E-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA: <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a> + 'static&gt; <a class=\"trait\" href=\"hotshot_types/vote/trait.Vote.html\" title=\"trait hotshot_types::vote::Vote\">Vote</a>&lt;TYPES&gt; for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle\" open><summary><section id=\"associatedtype.Commitment\" class=\"associatedtype trait-impl\"><a href=\"#associatedtype.Commitment\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a href=\"hotshot_types/vote/trait.Vote.html#associatedtype.Commitment\" class=\"associatedtype\">Commitment</a> = DATA</h4></section></summary><div class='docblock'>Type of data commitment this vote uses.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.get_signing_key\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#125-127\">source</a><a href=\"#method.get_signing_key\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"hotshot_types/vote/trait.Vote.html#tymethod.get_signing_key\" class=\"fn\">get_signing_key</a>(&amp;self) -&gt; &lt;TYPES as <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>&gt;::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a></h4></section></summary><div class='docblock'>Gets the public signature key of the votes creator/sender</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.get_signature\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#129-131\">source</a><a href=\"#method.get_signature\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"hotshot_types/vote/trait.Vote.html#tymethod.get_signature\" class=\"fn\">get_signature</a>(\n    &amp;self\n) -&gt; &lt;TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a> as <a class=\"trait\" href=\"hotshot_types/traits/signature_key/trait.SignatureKey.html\" title=\"trait hotshot_types::traits::signature_key::SignatureKey\">SignatureKey</a>&gt;::<a class=\"associatedtype\" href=\"hotshot_types/traits/signature_key/trait.SignatureKey.html#associatedtype.PureAssembledSignatureType\" title=\"type hotshot_types::traits::signature_key::SignatureKey::PureAssembledSignatureType\">PureAssembledSignatureType</a></h4></section></summary><div class='docblock'>Get the signature of the vote sender</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.get_data\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#133-135\">source</a><a href=\"#method.get_data\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"hotshot_types/vote/trait.Vote.html#tymethod.get_data\" class=\"fn\">get_data</a>(&amp;self) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.reference.html\">&amp;DATA</a></h4></section></summary><div class='docblock'>Gets the data which was voted on by this vote</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.get_data_commitment\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#137-139\">source</a><a href=\"#method.get_data_commitment\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"hotshot_types/vote/trait.Vote.html#tymethod.get_data_commitment\" class=\"fn\">get_data_commitment</a>(&amp;self) -&gt; Commitment&lt;DATA&gt;</h4></section></summary><div class='docblock'>Gets the Data commitment of the vote</div></details></div></details>","Vote<TYPES>","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Serialize-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#impl-Serialize-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA&gt; <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serialize.html\" title=\"trait serde::ser::Serialize\">Serialize</a> for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;<div class=\"where\">where\n    DATA: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serialize.html\" title=\"trait serde::ser::Serialize\">Serialize</a> + <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a>,\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.Time\" title=\"type hotshot_types::traits::node_implementation::NodeType::Time\">Time</a>: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serialize.html\" title=\"trait serde::ser::Serialize\">Serialize</a>,</div></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.serialize\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#method.serialize\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serialize.html#tymethod.serialize\" class=\"fn\">serialize</a>&lt;__S&gt;(&amp;self, __serializer: __S) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;__S::<a class=\"associatedtype\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serializer.html#associatedtype.Ok\" title=\"type serde::ser::Serializer::Ok\">Ok</a>, __S::<a class=\"associatedtype\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serializer.html#associatedtype.Error\" title=\"type serde::ser::Serializer::Error\">Error</a>&gt;<div class=\"where\">where\n    __S: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serializer.html\" title=\"trait serde::ser::Serializer\">Serializer</a>,</div></h4></section></summary><div class='docblock'>Serialize this value into the given Serde serializer. <a href=\"https://docs.rs/serde/1.0.196/serde/ser/trait.Serialize.html#tymethod.serialize\">Read more</a></div></details></div></details>","Serialize","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Debug-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#impl-Debug-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> + <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> + <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a> for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;<div class=\"where\">where\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a>,\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.Time\" title=\"type hotshot_types::traits::node_implementation::NodeType::Time\">Time</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html\" title=\"trait core::fmt::Debug\">Debug</a>,</div></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.fmt\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#method.fmt\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html#tymethod.fmt\" class=\"fn\">fmt</a>(&amp;self, f: &amp;mut <a class=\"struct\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/struct.Formatter.html\" title=\"struct core::fmt::Formatter\">Formatter</a>&lt;'_&gt;) -&gt; <a class=\"type\" href=\"https://doc.rust-lang.org/1.76.0/core/fmt/type.Result.html\" title=\"type core::fmt::Result\">Result</a></h4></section></summary><div class='docblock'>Formats the value using the given formatter. <a href=\"https://doc.rust-lang.org/1.76.0/core/fmt/trait.Debug.html#tymethod.fmt\">Read more</a></div></details></div></details>","Debug","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Clone-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#impl-Clone-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> + <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a> for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;<div class=\"where\">where\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a>,\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.Time\" title=\"type hotshot_types::traits::node_implementation::NodeType::Time\">Time</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html\" title=\"trait core::clone::Clone\">Clone</a>,</div></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.clone\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#method.clone\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#tymethod.clone\" class=\"fn\">clone</a>(&amp;self) -&gt; <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;</h4></section></summary><div class='docblock'>Returns a copy of the value. <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#tymethod.clone\">Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.clone_from\" class=\"method trait-impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/core/clone.rs.html#169\">source</a></span><a href=\"#method.clone_from\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#method.clone_from\" class=\"fn\">clone_from</a>(&amp;mut self, source: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.reference.html\">&amp;Self</a>)</h4></section></summary><div class='docblock'>Performs copy-assignment from <code>source</code>. <a href=\"https://doc.rust-lang.org/1.76.0/core/clone/trait.Clone.html#method.clone_from\">Read more</a></div></details></div></details>","Clone","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Hash-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#impl-Hash-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> + <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> + <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a> for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;<div class=\"where\">where\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a>,\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.Time\" title=\"type hotshot_types::traits::node_implementation::NodeType::Time\">Time</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hash.html\" title=\"trait core::hash::Hash\">Hash</a>,</div></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.hash\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#method.hash\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hash.html#tymethod.hash\" class=\"fn\">hash</a>&lt;__H: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hasher.html\" title=\"trait core::hash::Hasher\">Hasher</a>&gt;(&amp;self, state: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.reference.html\">&amp;mut __H</a>)</h4></section></summary><div class='docblock'>Feeds this value into the given <a href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hasher.html\" title=\"trait core::hash::Hasher\"><code>Hasher</code></a>. <a href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hash.html#tymethod.hash\">Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.hash_slice\" class=\"method trait-impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.3.0\">1.3.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/core/hash/mod.rs.html#238-240\">source</a></span><a href=\"#method.hash_slice\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hash.html#method.hash_slice\" class=\"fn\">hash_slice</a>&lt;H&gt;(data: &amp;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.slice.html\">[Self]</a>, state: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.reference.html\">&amp;mut H</a>)<div class=\"where\">where\n    H: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hasher.html\" title=\"trait core::hash::Hasher\">Hasher</a>,\n    Self: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.Sized.html\" title=\"trait core::marker::Sized\">Sized</a>,</div></h4></section></summary><div class='docblock'>Feeds a slice of this type into the given <a href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hasher.html\" title=\"trait core::hash::Hasher\"><code>Hasher</code></a>. <a href=\"https://doc.rust-lang.org/1.76.0/core/hash/trait.Hash.html#method.hash_slice\">Read more</a></div></details></div></details>","Hash","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-HasViewNumber%3CTYPES%3E-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#116-120\">source</a><a href=\"#impl-HasViewNumber%3CTYPES%3E-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA: <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a> + 'static&gt; <a class=\"trait\" href=\"hotshot_types/vote/trait.HasViewNumber.html\" title=\"trait hotshot_types::vote::HasViewNumber\">HasViewNumber</a>&lt;TYPES&gt; for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;</h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.get_view_number\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#117-119\">source</a><a href=\"#method.get_view_number\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"hotshot_types/vote/trait.HasViewNumber.html#tymethod.get_view_number\" class=\"fn\">get_view_number</a>(&amp;self) -&gt; &lt;TYPES as <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>&gt;::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.Time\" title=\"type hotshot_types::traits::node_implementation::NodeType::Time\">Time</a></h4></section></summary><div class='docblock'>Returns the view number the type refers to.</div></details></div></details>","HasViewNumber<TYPES>","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<section id=\"impl-Eq-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#impl-Eq-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.Eq.html\" title=\"trait core::cmp::Eq\">Eq</a> + <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.Eq.html\" title=\"trait core::cmp::Eq\">Eq</a> + <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.Eq.html\" title=\"trait core::cmp::Eq\">Eq</a> for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;<div class=\"where\">where\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.Eq.html\" title=\"trait core::cmp::Eq\">Eq</a>,\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.Time\" title=\"type hotshot_types::traits::node_implementation::NodeType::Time\">Time</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.Eq.html\" title=\"trait core::cmp::Eq\">Eq</a>,</div></h3></section>","Eq","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<section id=\"impl-StructuralPartialEq-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#impl-StructuralPartialEq-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA: <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.StructuralPartialEq.html\" title=\"trait core::marker::StructuralPartialEq\">StructuralPartialEq</a> for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;</h3></section>","StructuralPartialEq","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-PartialEq-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#impl-PartialEq-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a> + <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a> + <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a> for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;<div class=\"where\">where\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.SignatureKey\" title=\"type hotshot_types::traits::node_implementation::NodeType::SignatureKey\">SignatureKey</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a>,\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.Time\" title=\"type hotshot_types::traits::node_implementation::NodeType::Time\">Time</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.PartialEq.html\" title=\"trait core::cmp::PartialEq\">PartialEq</a>,</div></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.eq\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#method.eq\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.PartialEq.html#tymethod.eq\" class=\"fn\">eq</a>(&amp;self, other: &amp;<a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>This method tests for <code>self</code> and <code>other</code> values to be equal, and is used\nby <code>==</code>.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.ne\" class=\"method trait-impl\"><span class=\"rightside\"><span class=\"since\" title=\"Stable since Rust version 1.0.0\">1.0.0</span> · <a class=\"src\" href=\"https://doc.rust-lang.org/1.76.0/src/core/cmp.rs.html#242\">source</a></span><a href=\"#method.ne\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://doc.rust-lang.org/1.76.0/core/cmp/trait.PartialEq.html#method.ne\" class=\"fn\">ne</a>(&amp;self, other: <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.reference.html\">&amp;Rhs</a>) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.76.0/std/primitive.bool.html\">bool</a></h4></section></summary><div class='docblock'>This method tests for <code>!=</code>. The default implementation is almost always\nsufficient, and should not be overridden without very good reason.</div></details></div></details>","PartialEq","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Deserialize%3C'de%3E-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#impl-Deserialize%3C'de%3E-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;'de, TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA&gt; <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserialize.html\" title=\"trait serde::de::Deserialize\">Deserialize</a>&lt;'de&gt; for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;<div class=\"where\">where\n    DATA: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserialize.html\" title=\"trait serde::de::Deserialize\">Deserialize</a>&lt;'de&gt; + <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a>,\n    TYPES::<a class=\"associatedtype\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html#associatedtype.Time\" title=\"type hotshot_types::traits::node_implementation::NodeType::Time\">Time</a>: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserialize.html\" title=\"trait serde::de::Deserialize\">Deserialize</a>&lt;'de&gt;,</div></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.deserialize\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#method.deserialize\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserialize.html#tymethod.deserialize\" class=\"fn\">deserialize</a>&lt;__D&gt;(__deserializer: __D) -&gt; <a class=\"enum\" href=\"https://doc.rust-lang.org/1.76.0/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;Self, __D::<a class=\"associatedtype\" href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserializer.html#associatedtype.Error\" title=\"type serde::de::Deserializer::Error\">Error</a>&gt;<div class=\"where\">where\n    __D: <a class=\"trait\" href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserializer.html\" title=\"trait serde::de::Deserializer\">Deserializer</a>&lt;'de&gt;,</div></h4></section></summary><div class='docblock'>Deserialize this value from the given Serde deserializer. <a href=\"https://docs.rs/serde/1.0.196/serde/de/trait.Deserialize.html#tymethod.deserialize\">Read more</a></div></details></div></details>","Deserialize<'de>","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"],["<section id=\"impl-StructuralEq-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/simple_vote.rs.html#103\">source</a><a href=\"#impl-StructuralEq-for-SimpleVote%3CTYPES,+DATA%3E\" class=\"anchor\">§</a><h3 class=\"code-header\">impl&lt;TYPES: <a class=\"trait\" href=\"hotshot_types/traits/node_implementation/trait.NodeType.html\" title=\"trait hotshot_types::traits::node_implementation::NodeType\">NodeType</a>, DATA: <a class=\"trait\" href=\"hotshot_types/simple_vote/trait.Voteable.html\" title=\"trait hotshot_types::simple_vote::Voteable\">Voteable</a>&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/1.76.0/core/marker/trait.StructuralEq.html\" title=\"trait core::marker::StructuralEq\">StructuralEq</a> for <a class=\"struct\" href=\"hotshot_types/simple_vote/struct.SimpleVote.html\" title=\"struct hotshot_types::simple_vote::SimpleVote\">SimpleVote</a>&lt;TYPES, DATA&gt;</h3></section>","StructuralEq","hotshot_types::simple_vote::QuorumVote","hotshot_types::simple_vote::DAVote","hotshot_types::simple_vote::TimeoutVote","hotshot_types::simple_vote::ViewSyncCommitVote","hotshot_types::simple_vote::ViewSyncPreCommitVote","hotshot_types::simple_vote::ViewSyncFinalizeVote","hotshot_types::simple_vote::UpgradeVote"]]
};if (window.register_type_impls) {window.register_type_impls(type_impls);} else {window.pending_type_impls = type_impls;}})()