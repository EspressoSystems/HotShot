(function() {var type_impls = {
"hotshot_types":[["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-VidScheme-for-VidSchemeType\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#143-190\">source</a><a href=\"#impl-VidScheme-for-VidSchemeType\" class=\"anchor\">§</a><h3 class=\"code-header\">impl VidScheme for <a class=\"struct\" href=\"hotshot_types/vid/struct.VidSchemeType.html\" title=\"struct hotshot_types::vid::VidSchemeType\">VidSchemeType</a></h3></section></summary><div class=\"impl-items\"><details class=\"toggle\" open><summary><section id=\"associatedtype.Commit\" class=\"associatedtype trait-impl\"><a href=\"#associatedtype.Commit\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a class=\"associatedtype\">Commit</a> = &lt;AdvzInternal&lt;Bn&lt;Config&gt;, CoreWrapper&lt;CtVariableCoreWrapper&lt;Sha256VarCore, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UTerm.html\" title=\"struct typenum::uint::UTerm\">UTerm</a>, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B1.html\" title=\"struct typenum::bit::B1\">B1</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, OidSha256&gt;&gt;, <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>&gt; as VidScheme&gt;::Commit</h4></section></summary><div class='docblock'>Payload commitment.</div></details><details class=\"toggle\" open><summary><section id=\"associatedtype.Share\" class=\"associatedtype trait-impl\"><a href=\"#associatedtype.Share\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a class=\"associatedtype\">Share</a> = &lt;AdvzInternal&lt;Bn&lt;Config&gt;, CoreWrapper&lt;CtVariableCoreWrapper&lt;Sha256VarCore, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UTerm.html\" title=\"struct typenum::uint::UTerm\">UTerm</a>, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B1.html\" title=\"struct typenum::bit::B1\">B1</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, OidSha256&gt;&gt;, <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>&gt; as VidScheme&gt;::Share</h4></section></summary><div class='docblock'>Share-specific data sent to a storage node.</div></details><details class=\"toggle\" open><summary><section id=\"associatedtype.Common\" class=\"associatedtype trait-impl\"><a href=\"#associatedtype.Common\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a class=\"associatedtype\">Common</a> = &lt;AdvzInternal&lt;Bn&lt;Config&gt;, CoreWrapper&lt;CtVariableCoreWrapper&lt;Sha256VarCore, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UTerm.html\" title=\"struct typenum::uint::UTerm\">UTerm</a>, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B1.html\" title=\"struct typenum::bit::B1\">B1</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, OidSha256&gt;&gt;, <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>&gt; as VidScheme&gt;::Common</h4></section></summary><div class='docblock'>Common data sent to all storage nodes.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.commit_only\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#148-153\">source</a><a href=\"#method.commit_only\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">commit_only</a>&lt;B&gt;(&amp;mut self, payload: B) -&gt; VidResult&lt;Self::Commit&gt;<div class=\"where\">where\n    B: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.77.1/core/convert/trait.AsRef.html\" title=\"trait core::convert::AsRef\">AsRef</a>&lt;[<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.u8.html\">u8</a>]&gt;,</div></h4></section></summary><div class='docblock'>Compute a payload commitment</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.disperse\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#155-160\">source</a><a href=\"#method.disperse\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">disperse</a>&lt;B&gt;(&amp;mut self, payload: B) -&gt; VidResult&lt;VidDisperse&lt;Self&gt;&gt;<div class=\"where\">where\n    B: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.77.1/core/convert/trait.AsRef.html\" title=\"trait core::convert::AsRef\">AsRef</a>&lt;[<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.u8.html\">u8</a>]&gt;,</div></h4></section></summary><div class='docblock'>Compute shares to send to the storage nodes</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.verify_share\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#162-169\">source</a><a href=\"#method.verify_share\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">verify_share</a>(\n    &amp;self,\n    share: &amp;Self::Share,\n    common: &amp;Self::Common,\n    commit: &amp;Self::Commit\n) -&gt; VidResult&lt;<a class=\"enum\" href=\"https://doc.rust-lang.org/1.77.1/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>, <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>&gt;&gt;</h4></section></summary><div class='docblock'>Verify a share. Used by both storage node and retrieval client.\nWhy is return type a nested <code>Result</code>? See <a href=\"https://sled.rs/errors\">https://sled.rs/errors</a>\nReturns: <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.recover_payload\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#171-173\">source</a><a href=\"#method.recover_payload\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">recover_payload</a>(\n    &amp;self,\n    shares: &amp;[Self::Share],\n    common: &amp;Self::Common\n) -&gt; VidResult&lt;<a class=\"struct\" href=\"https://doc.rust-lang.org/1.77.1/alloc/vec/struct.Vec.html\" title=\"struct alloc::vec::Vec\">Vec</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.u8.html\">u8</a>&gt;&gt;</h4></section></summary><div class='docblock'>Recover payload from shares.\nDo not verify shares or check recovered payload against anything.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.is_consistent\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#175-177\">source</a><a href=\"#method.is_consistent\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">is_consistent</a>(commit: &amp;Self::Commit, common: &amp;Self::Common) -&gt; VidResult&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>&gt;</h4></section></summary><div class='docblock'>Check that a [<code>VidScheme::Common</code>] is consistent with a\n[<code>VidScheme::Commit</code>]. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.get_payload_byte_len\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#179-181\">source</a><a href=\"#method.get_payload_byte_len\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">get_payload_byte_len</a>(common: &amp;Self::Common) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.u32.html\">u32</a></h4></section></summary><div class='docblock'>Extract the payload byte length data from a [<code>VidScheme::Common</code>].</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.get_num_storage_nodes\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#183-185\">source</a><a href=\"#method.get_num_storage_nodes\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">get_num_storage_nodes</a>(common: &amp;Self::Common) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.u32.html\">u32</a></h4></section></summary><div class='docblock'>Extract the number of storage nodes from a [<code>VidScheme::Common</code>].</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.get_multiplicity\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#187-189\">source</a><a href=\"#method.get_multiplicity\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">get_multiplicity</a>(common: &amp;Self::Common) -&gt; <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.u32.html\">u32</a></h4></section></summary><div class='docblock'>Extract the number of poly evals per share [<code>VidScheme::Common</code>].</div></details></div></details>","VidScheme","hotshot_types::vid::VidCommitment","hotshot_types::vid::VidCommon","hotshot_types::vid::VidShare"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-Precomputable-for-VidSchemeType\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#230-255\">source</a><a href=\"#impl-Precomputable-for-VidSchemeType\" class=\"anchor\">§</a><h3 class=\"code-header\">impl Precomputable for <a class=\"struct\" href=\"hotshot_types/vid/struct.VidSchemeType.html\" title=\"struct hotshot_types::vid::VidSchemeType\">VidSchemeType</a></h3></section></summary><div class=\"impl-items\"><details class=\"toggle\" open><summary><section id=\"associatedtype.PrecomputeData\" class=\"associatedtype trait-impl\"><a href=\"#associatedtype.PrecomputeData\" class=\"anchor\">§</a><h4 class=\"code-header\">type <a class=\"associatedtype\">PrecomputeData</a> = &lt;AdvzInternal&lt;Bn&lt;Config&gt;, CoreWrapper&lt;CtVariableCoreWrapper&lt;Sha256VarCore, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UInt.html\" title=\"struct typenum::uint::UInt\">UInt</a>&lt;<a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/uint/struct.UTerm.html\" title=\"struct typenum::uint::UTerm\">UTerm</a>, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B1.html\" title=\"struct typenum::bit::B1\">B1</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, <a class=\"struct\" href=\"https://docs.rs/typenum/1.17.0/typenum/bit/struct.B0.html\" title=\"struct typenum::bit::B0\">B0</a>&gt;, OidSha256&gt;&gt;, <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>&gt; as Precomputable&gt;::PrecomputeData</h4></section></summary><div class='docblock'>Precomputed data that can be (re-)used during disperse computation</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.commit_only_precompute\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#233-241\">source</a><a href=\"#method.commit_only_precompute\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">commit_only_precompute</a>&lt;B&gt;(\n    &amp;self,\n    payload: B\n) -&gt; VidResult&lt;(Self::Commit, Self::PrecomputeData)&gt;<div class=\"where\">where\n    B: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.77.1/core/convert/trait.AsRef.html\" title=\"trait core::convert::AsRef\">AsRef</a>&lt;[<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.u8.html\">u8</a>]&gt;,</div></h4></section></summary><div class='docblock'>Similar to [<code>VidScheme::commit_only</code>] but returns additional data that\ncan be used as input to <code>disperse_precompute</code> for faster dispersal.</div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.disperse_precompute\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#243-254\">source</a><a href=\"#method.disperse_precompute\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">disperse_precompute</a>&lt;B&gt;(\n    &amp;self,\n    payload: B,\n    data: &amp;Self::PrecomputeData\n) -&gt; VidResult&lt;VidDisperse&lt;Self&gt;&gt;<div class=\"where\">where\n    B: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.77.1/core/convert/trait.AsRef.html\" title=\"trait core::convert::AsRef\">AsRef</a>&lt;[<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.u8.html\">u8</a>]&gt;,</div></h4></section></summary><div class='docblock'>Similar to [<code>VidScheme::disperse</code>] but takes as input additional\ndata for more efficient computation and faster disersal.</div></details></div></details>","Precomputable","hotshot_types::vid::VidCommitment","hotshot_types::vid::VidCommon","hotshot_types::vid::VidShare"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-PayloadProver%3CSmallRangeProofType%3E-for-VidSchemeType\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#211-228\">source</a><a href=\"#impl-PayloadProver%3CSmallRangeProofType%3E-for-VidSchemeType\" class=\"anchor\">§</a><h3 class=\"code-header\">impl PayloadProver&lt;<a class=\"struct\" href=\"hotshot_types/vid/struct.SmallRangeProofType.html\" title=\"struct hotshot_types::vid::SmallRangeProofType\">SmallRangeProofType</a>&gt; for <a class=\"struct\" href=\"hotshot_types/vid/struct.VidSchemeType.html\" title=\"struct hotshot_types::vid::VidSchemeType\">VidSchemeType</a></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.payload_proof\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#212-219\">source</a><a href=\"#method.payload_proof\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">payload_proof</a>&lt;B&gt;(\n    &amp;self,\n    payload: B,\n    range: <a class=\"struct\" href=\"https://doc.rust-lang.org/1.77.1/core/ops/range/struct.Range.html\" title=\"struct core::ops::range::Range\">Range</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.usize.html\">usize</a>&gt;\n) -&gt; VidResult&lt;<a class=\"struct\" href=\"hotshot_types/vid/struct.SmallRangeProofType.html\" title=\"struct hotshot_types::vid::SmallRangeProofType\">SmallRangeProofType</a>&gt;<div class=\"where\">where\n    B: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.77.1/core/convert/trait.AsRef.html\" title=\"trait core::convert::AsRef\">AsRef</a>&lt;[<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.u8.html\">u8</a>]&gt;,</div></h4></section></summary><div class='docblock'>Compute a proof for a subslice of payload data. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.payload_verify\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#221-227\">source</a><a href=\"#method.payload_verify\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">payload_verify</a>(\n    &amp;self,\n    stmt: Statement&lt;'_, Self&gt;,\n    proof: &amp;<a class=\"struct\" href=\"hotshot_types/vid/struct.SmallRangeProofType.html\" title=\"struct hotshot_types::vid::SmallRangeProofType\">SmallRangeProofType</a>\n) -&gt; VidResult&lt;<a class=\"enum\" href=\"https://doc.rust-lang.org/1.77.1/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>, <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>&gt;&gt;</h4></section></summary><div class='docblock'>Verify a proof made by [<code>PayloadProver::payload_proof</code>]. <a>Read more</a></div></details></div></details>","PayloadProver<SmallRangeProofType>","hotshot_types::vid::VidCommitment","hotshot_types::vid::VidCommon","hotshot_types::vid::VidShare"],["<details class=\"toggle implementors-toggle\" open><summary><section id=\"impl-PayloadProver%3CLargeRangeProofType%3E-for-VidSchemeType\" class=\"impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#192-209\">source</a><a href=\"#impl-PayloadProver%3CLargeRangeProofType%3E-for-VidSchemeType\" class=\"anchor\">§</a><h3 class=\"code-header\">impl PayloadProver&lt;<a class=\"struct\" href=\"hotshot_types/vid/struct.LargeRangeProofType.html\" title=\"struct hotshot_types::vid::LargeRangeProofType\">LargeRangeProofType</a>&gt; for <a class=\"struct\" href=\"hotshot_types/vid/struct.VidSchemeType.html\" title=\"struct hotshot_types::vid::VidSchemeType\">VidSchemeType</a></h3></section></summary><div class=\"impl-items\"><details class=\"toggle method-toggle\" open><summary><section id=\"method.payload_proof\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#193-200\">source</a><a href=\"#method.payload_proof\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">payload_proof</a>&lt;B&gt;(\n    &amp;self,\n    payload: B,\n    range: <a class=\"struct\" href=\"https://doc.rust-lang.org/1.77.1/core/ops/range/struct.Range.html\" title=\"struct core::ops::range::Range\">Range</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.usize.html\">usize</a>&gt;\n) -&gt; VidResult&lt;<a class=\"struct\" href=\"hotshot_types/vid/struct.LargeRangeProofType.html\" title=\"struct hotshot_types::vid::LargeRangeProofType\">LargeRangeProofType</a>&gt;<div class=\"where\">where\n    B: <a class=\"trait\" href=\"https://doc.rust-lang.org/1.77.1/core/convert/trait.AsRef.html\" title=\"trait core::convert::AsRef\">AsRef</a>&lt;[<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.u8.html\">u8</a>]&gt;,</div></h4></section></summary><div class='docblock'>Compute a proof for a subslice of payload data. <a>Read more</a></div></details><details class=\"toggle method-toggle\" open><summary><section id=\"method.payload_verify\" class=\"method trait-impl\"><a class=\"src rightside\" href=\"src/hotshot_types/vid.rs.html#202-208\">source</a><a href=\"#method.payload_verify\" class=\"anchor\">§</a><h4 class=\"code-header\">fn <a class=\"fn\">payload_verify</a>(\n    &amp;self,\n    stmt: Statement&lt;'_, Self&gt;,\n    proof: &amp;<a class=\"struct\" href=\"hotshot_types/vid/struct.LargeRangeProofType.html\" title=\"struct hotshot_types::vid::LargeRangeProofType\">LargeRangeProofType</a>\n) -&gt; VidResult&lt;<a class=\"enum\" href=\"https://doc.rust-lang.org/1.77.1/core/result/enum.Result.html\" title=\"enum core::result::Result\">Result</a>&lt;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>, <a class=\"primitive\" href=\"https://doc.rust-lang.org/1.77.1/std/primitive.unit.html\">()</a>&gt;&gt;</h4></section></summary><div class='docblock'>Verify a proof made by [<code>PayloadProver::payload_proof</code>]. <a>Read more</a></div></details></div></details>","PayloadProver<LargeRangeProofType>","hotshot_types::vid::VidCommitment","hotshot_types::vid::VidCommon","hotshot_types::vid::VidShare"]]
};if (window.register_type_impls) {window.register_type_impls(type_impls);} else {window.pending_type_impls = type_impls;}})()