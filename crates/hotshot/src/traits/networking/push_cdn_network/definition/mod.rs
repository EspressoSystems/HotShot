use std::marker::PhantomData;

use cdn_broker::reexports::{
    connection::protocols::{Quic, Tcp},
    def::{hook::NoMessageHook, ConnectionDef, RunDef, Topic as TopicTrait},
    discovery::{Embedded, Redis},
};
use hotshot_types::traits::{node_implementation::NodeType, signature_key::SignatureKey};
use message_hook::HotShotMessageHook;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use signature_key::WrappedSignatureKey;

pub mod message_hook;
pub mod signature_key;

/// The enum for the topics we can subscribe to in the Push CDN
#[repr(u8)]
#[derive(IntoPrimitive, TryFromPrimitive, Clone, PartialEq, Eq)]
pub enum Topic {
    /// The global topic
    Global = 0,
    /// The DA topic
    Da = 1,
}

/// Implement the `TopicTrait` for our `Topic` enum. We need this to filter
/// topics that are not implemented at the application level.
impl TopicTrait for Topic {}

/// The production run definition for the Push CDN.
/// Uses the real protocols and a Redis discovery client.
pub struct ProductionDef<T: NodeType>(PhantomData<T>);
impl<T: NodeType> RunDef for ProductionDef<T> {
    type User = UserDef<T>;
    type Broker = BrokerDef<T::SignatureKey>;
    type DiscoveryClientType = Redis;
    type Topic = Topic;
}

/// The user definition for the Push CDN.
/// Uses the Quic protocol and untrusted middleware.
pub struct UserDef<T: NodeType>(PhantomData<T>);
impl<T: NodeType> ConnectionDef for UserDef<T> {
    type Scheme = WrappedSignatureKey<T::SignatureKey>;
    type Protocol = Quic;
    type MessageHook = HotShotMessageHook<T>;
}

/// The broker definition for the Push CDN.
/// Uses the TCP protocol and trusted middleware.
pub struct BrokerDef<K: SignatureKey + 'static>(PhantomData<K>);
impl<K: SignatureKey> ConnectionDef for BrokerDef<K> {
    type Scheme = WrappedSignatureKey<K>;
    type Protocol = Tcp;
    type MessageHook = NoMessageHook;
}

/// The client definition for the Push CDN. Uses the Quic
/// protocol and no middleware. Differs from the user
/// definition in that is on the client-side.
#[derive(Clone)]
pub struct ClientDef<K: SignatureKey + 'static>(PhantomData<K>);
impl<K: SignatureKey> ConnectionDef for ClientDef<K> {
    type Scheme = WrappedSignatureKey<K>;
    type Protocol = Quic;
    type MessageHook = NoMessageHook;
}

/// The testing run definition for the Push CDN.
/// Uses the real protocols, but with an embedded discovery clientn.
pub struct TestingDef<T: NodeType>(PhantomData<T>);
impl<T: NodeType> RunDef for TestingDef<T> {
    type User = UserDef<T>;
    type Broker = BrokerDef<T::SignatureKey>;
    type DiscoveryClientType = Embedded;
    type Topic = Topic;
}
