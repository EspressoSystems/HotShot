(function() {var implementors = {};
implementors["phaselock"] = [{"text":"impl&lt;S, const N:&nbsp;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.58.1/std/primitive.usize.html\">usize</a>&gt; Freeze for <a class=\"struct\" href=\"phaselock/committee/struct.DynamicCommittee.html\" title=\"struct phaselock::committee::DynamicCommittee\">DynamicCommittee</a>&lt;S, N&gt;","synthetic":true,"types":["phaselock::committee::DynamicCommittee"]},{"text":"impl Freeze for <a class=\"struct\" href=\"phaselock/demos/dentry/struct.Subtraction.html\" title=\"struct phaselock::demos::dentry::Subtraction\">Subtraction</a>","synthetic":true,"types":["phaselock::demos::dentry::Subtraction"]},{"text":"impl Freeze for <a class=\"struct\" href=\"phaselock/demos/dentry/struct.Addition.html\" title=\"struct phaselock::demos::dentry::Addition\">Addition</a>","synthetic":true,"types":["phaselock::demos::dentry::Addition"]},{"text":"impl Freeze for <a class=\"enum\" href=\"phaselock/demos/dentry/enum.DEntryError.html\" title=\"enum phaselock::demos::dentry::DEntryError\">DEntryError</a>","synthetic":true,"types":["phaselock::demos::dentry::DEntryError"]},{"text":"impl Freeze for <a class=\"struct\" href=\"phaselock/demos/dentry/struct.Transaction.html\" title=\"struct phaselock::demos::dentry::Transaction\">Transaction</a>","synthetic":true,"types":["phaselock::demos::dentry::Transaction"]},{"text":"impl Freeze for <a class=\"struct\" href=\"phaselock/demos/dentry/struct.State.html\" title=\"struct phaselock::demos::dentry::State\">State</a>","synthetic":true,"types":["phaselock::demos::dentry::State"]},{"text":"impl Freeze for <a class=\"struct\" href=\"phaselock/demos/dentry/struct.DEntryBlock.html\" title=\"struct phaselock::demos::dentry::DEntryBlock\">DEntryBlock</a>","synthetic":true,"types":["phaselock::demos::dentry::DEntryBlock"]},{"text":"impl&lt;NET&gt; Freeze for <a class=\"struct\" href=\"phaselock/demos/dentry/struct.DEntryNode.html\" title=\"struct phaselock::demos::dentry::DEntryNode\">DEntryNode</a>&lt;NET&gt;","synthetic":true,"types":["phaselock::demos::dentry::DEntryNode"]},{"text":"impl&lt;I, const N:&nbsp;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.58.1/std/primitive.usize.html\">usize</a>&gt; Freeze for <a class=\"struct\" href=\"phaselock/state_machine/struct.SequentialRound.html\" title=\"struct phaselock::state_machine::SequentialRound\">SequentialRound</a>&lt;I, N&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;&lt;I as <a class=\"trait\" href=\"phaselock/traits/trait.NodeImplementation.html\" title=\"trait phaselock::traits::NodeImplementation\">NodeImplementation</a>&lt;N&gt;&gt;::<a class=\"type\" href=\"phaselock/traits/trait.NodeImplementation.html#associatedtype.Block\" title=\"type phaselock::traits::NodeImplementation::Block\">Block</a>: Freeze,<br>&nbsp;&nbsp;&nbsp;&nbsp;&lt;I as <a class=\"trait\" href=\"phaselock/traits/trait.NodeImplementation.html\" title=\"trait phaselock::traits::NodeImplementation\">NodeImplementation</a>&lt;N&gt;&gt;::<a class=\"type\" href=\"phaselock/traits/trait.NodeImplementation.html#associatedtype.State\" title=\"type phaselock::traits::NodeImplementation::State\">State</a>: Freeze,&nbsp;</span>","synthetic":true,"types":["phaselock::state_machine::SequentialRound"]},{"text":"impl&lt;S, const N:&nbsp;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.58.1/std/primitive.usize.html\">usize</a>&gt; Freeze for <a class=\"struct\" href=\"phaselock/traits/election/struct.StaticCommittee.html\" title=\"struct phaselock::traits::election::StaticCommittee\">StaticCommittee</a>&lt;S, N&gt;","synthetic":true,"types":["phaselock::traits::election::StaticCommittee"]},{"text":"impl Freeze for <a class=\"struct\" href=\"phaselock/traits/implementations/struct.DummyReliability.html\" title=\"struct phaselock::traits::implementations::DummyReliability\">DummyReliability</a>","synthetic":true,"types":["phaselock::traits::networking::memory_network::DummyReliability"]},{"text":"impl&lt;T&gt; Freeze for <a class=\"struct\" href=\"phaselock/traits/implementations/struct.MasterMap.html\" title=\"struct phaselock::traits::implementations::MasterMap\">MasterMap</a>&lt;T&gt;","synthetic":true,"types":["phaselock::traits::networking::memory_network::MasterMap"]},{"text":"impl&lt;T&gt; Freeze for <a class=\"struct\" href=\"phaselock/traits/implementations/struct.MemoryNetwork.html\" title=\"struct phaselock::traits::implementations::MemoryNetwork\">MemoryNetwork</a>&lt;T&gt;","synthetic":true,"types":["phaselock::traits::networking::memory_network::MemoryNetwork"]},{"text":"impl&lt;T&gt; Freeze for <a class=\"struct\" href=\"phaselock/traits/implementations/struct.WNetwork.html\" title=\"struct phaselock::traits::implementations::WNetwork\">WNetwork</a>&lt;T&gt;","synthetic":true,"types":["phaselock::traits::networking::w_network::WNetwork"]},{"text":"impl&lt;Block, State, const N:&nbsp;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.58.1/std/primitive.usize.html\">usize</a>&gt; Freeze for <a class=\"struct\" href=\"phaselock/traits/implementations/struct.MemoryStorage.html\" title=\"struct phaselock::traits::implementations::MemoryStorage\">MemoryStorage</a>&lt;Block, State, N&gt;","synthetic":true,"types":["phaselock::traits::storage::memory_storage::MemoryStorage"]},{"text":"impl&lt;I, const N:&nbsp;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.58.1/std/primitive.usize.html\">usize</a>&gt; Freeze for <a class=\"struct\" href=\"phaselock/types/struct.PhaseLockHandle.html\" title=\"struct phaselock::types::PhaseLockHandle\">PhaseLockHandle</a>&lt;I, N&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;&lt;I as <a class=\"trait\" href=\"phaselock/traits/trait.NodeImplementation.html\" title=\"trait phaselock::traits::NodeImplementation\">NodeImplementation</a>&lt;N&gt;&gt;::<a class=\"type\" href=\"phaselock/traits/trait.NodeImplementation.html#associatedtype.Storage\" title=\"type phaselock::traits::NodeImplementation::Storage\">Storage</a>: Freeze,&nbsp;</span>","synthetic":true,"types":["phaselock::types::handle::PhaseLockHandle"]},{"text":"impl&lt;T&gt; Freeze for <a class=\"struct\" href=\"phaselock/utility/broadcast/struct.BroadcastSender.html\" title=\"struct phaselock::utility::broadcast::BroadcastSender\">BroadcastSender</a>&lt;T&gt;","synthetic":true,"types":["phaselock::utility::broadcast::BroadcastSender"]},{"text":"impl&lt;T&gt; Freeze for <a class=\"struct\" href=\"phaselock/utility/broadcast/struct.BroadcastReceiver.html\" title=\"struct phaselock::utility::broadcast::BroadcastReceiver\">BroadcastReceiver</a>&lt;T&gt;","synthetic":true,"types":["phaselock::utility::broadcast::BroadcastReceiver"]},{"text":"impl&lt;T&gt; !Freeze for <a class=\"struct\" href=\"phaselock/utility/subscribable_mutex/struct.SubscribableMutex.html\" title=\"struct phaselock::utility::subscribable_mutex::SubscribableMutex\">SubscribableMutex</a>&lt;T&gt;","synthetic":true,"types":["phaselock::utility::subscribable_mutex::SubscribableMutex"]},{"text":"impl&lt;T&gt; !Freeze for <a class=\"struct\" href=\"phaselock/utility/waitqueue/struct.WaitQueue.html\" title=\"struct phaselock::utility::waitqueue::WaitQueue\">WaitQueue</a>&lt;T&gt;","synthetic":true,"types":["phaselock::utility::waitqueue::WaitQueue"]},{"text":"impl&lt;T&gt; !Freeze for <a class=\"struct\" href=\"phaselock/utility/waitqueue/struct.WaitOnce.html\" title=\"struct phaselock::utility::waitqueue::WaitOnce\">WaitOnce</a>&lt;T&gt;","synthetic":true,"types":["phaselock::utility::waitqueue::WaitOnce"]},{"text":"impl Freeze for <a class=\"struct\" href=\"phaselock/struct.PhaseLockConfig.html\" title=\"struct phaselock::PhaseLockConfig\">PhaseLockConfig</a>","synthetic":true,"types":["phaselock::PhaseLockConfig"]},{"text":"impl&lt;I, const N:&nbsp;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.58.1/std/primitive.usize.html\">usize</a>&gt; !Freeze for <a class=\"struct\" href=\"phaselock/struct.PhaseLockInner.html\" title=\"struct phaselock::PhaseLockInner\">PhaseLockInner</a>&lt;I, N&gt;","synthetic":true,"types":["phaselock::PhaseLockInner"]},{"text":"impl&lt;I, const N:&nbsp;<a class=\"primitive\" href=\"https://doc.rust-lang.org/1.58.1/std/primitive.usize.html\">usize</a>&gt; Freeze for <a class=\"struct\" href=\"phaselock/struct.PhaseLock.html\" title=\"struct phaselock::PhaseLock\">PhaseLock</a>&lt;I, N&gt;","synthetic":true,"types":["phaselock::PhaseLock"]}];
implementors["threshold_crypto"] = [{"text":"impl Freeze for <a class=\"enum\" href=\"threshold_crypto/error/enum.Error.html\" title=\"enum threshold_crypto::error::Error\">Error</a>","synthetic":true,"types":["threshold_crypto::error::Error"]},{"text":"impl Freeze for <a class=\"enum\" href=\"threshold_crypto/error/enum.FromBytesError.html\" title=\"enum threshold_crypto::error::FromBytesError\">FromBytesError</a>","synthetic":true,"types":["threshold_crypto::error::FromBytesError"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/poly/struct.Poly.html\" title=\"struct threshold_crypto::poly::Poly\">Poly</a>","synthetic":true,"types":["threshold_crypto::poly::Poly"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/poly/struct.Commitment.html\" title=\"struct threshold_crypto::poly::Commitment\">Commitment</a>","synthetic":true,"types":["threshold_crypto::poly::Commitment"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/poly/struct.BivarPoly.html\" title=\"struct threshold_crypto::poly::BivarPoly\">BivarPoly</a>","synthetic":true,"types":["threshold_crypto::poly::BivarPoly"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/poly/struct.BivarCommitment.html\" title=\"struct threshold_crypto::poly::BivarCommitment\">BivarCommitment</a>","synthetic":true,"types":["threshold_crypto::poly::BivarCommitment"]},{"text":"impl&lt;B&gt; Freeze for <a class=\"struct\" href=\"threshold_crypto/serde_impl/struct.FieldWrap.html\" title=\"struct threshold_crypto::serde_impl::FieldWrap\">FieldWrap</a>&lt;B&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;B: Freeze,&nbsp;</span>","synthetic":true,"types":["threshold_crypto::serde_impl::field_vec::FieldWrap"]},{"text":"impl&lt;T&gt; Freeze for <a class=\"struct\" href=\"threshold_crypto/serde_impl/struct.SerdeSecret.html\" title=\"struct threshold_crypto::serde_impl::SerdeSecret\">SerdeSecret</a>&lt;T&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;T: Freeze,&nbsp;</span>","synthetic":true,"types":["threshold_crypto::serde_impl::SerdeSecret"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.Fr.html\" title=\"struct threshold_crypto::Fr\">Mersenne8</a>","synthetic":true,"types":["threshold_crypto::mock::ms8::Mersenne8"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.PEngine.html\" title=\"struct threshold_crypto::PEngine\">Mocktography</a>","synthetic":true,"types":["threshold_crypto::mock::Mocktography"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.G1Affine.html\" title=\"struct threshold_crypto::G1Affine\">Ms8Affine</a>","synthetic":true,"types":["threshold_crypto::mock::Ms8Affine"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.G1.html\" title=\"struct threshold_crypto::G1\">Ms8Projective</a>","synthetic":true,"types":["threshold_crypto::mock::Ms8Projective"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.PublicKey.html\" title=\"struct threshold_crypto::PublicKey\">PublicKey</a>","synthetic":true,"types":["threshold_crypto::PublicKey"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.PublicKeyShare.html\" title=\"struct threshold_crypto::PublicKeyShare\">PublicKeyShare</a>","synthetic":true,"types":["threshold_crypto::PublicKeyShare"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.Signature.html\" title=\"struct threshold_crypto::Signature\">Signature</a>","synthetic":true,"types":["threshold_crypto::Signature"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.SignatureShare.html\" title=\"struct threshold_crypto::SignatureShare\">SignatureShare</a>","synthetic":true,"types":["threshold_crypto::SignatureShare"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.SecretKey.html\" title=\"struct threshold_crypto::SecretKey\">SecretKey</a>","synthetic":true,"types":["threshold_crypto::SecretKey"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.SecretKeyShare.html\" title=\"struct threshold_crypto::SecretKeyShare\">SecretKeyShare</a>","synthetic":true,"types":["threshold_crypto::SecretKeyShare"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.Ciphertext.html\" title=\"struct threshold_crypto::Ciphertext\">Ciphertext</a>","synthetic":true,"types":["threshold_crypto::Ciphertext"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.DecryptionShare.html\" title=\"struct threshold_crypto::DecryptionShare\">DecryptionShare</a>","synthetic":true,"types":["threshold_crypto::DecryptionShare"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.PublicKeySet.html\" title=\"struct threshold_crypto::PublicKeySet\">PublicKeySet</a>","synthetic":true,"types":["threshold_crypto::PublicKeySet"]},{"text":"impl Freeze for <a class=\"struct\" href=\"threshold_crypto/struct.SecretKeySet.html\" title=\"struct threshold_crypto::SecretKeySet\">SecretKeySet</a>","synthetic":true,"types":["threshold_crypto::SecretKeySet"]}];
if (window.register_implementors) {window.register_implementors(implementors);} else {window.pending_implementors = implementors;}})()