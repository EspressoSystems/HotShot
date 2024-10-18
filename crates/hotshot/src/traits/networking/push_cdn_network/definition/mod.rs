use std::marker::PhantomData;

use cdn_broker::reexports::{
    connection::protocols::{Tcp, TcpTls},
    def::{hook::NoMessageHook, ConnectionDef, RunDef, Topic as TopicTrait},
    discovery::{Embedded, Redis},
};
use hotshot_types::traits::{network::Topic as HotShotTopic, signature_key::SignatureKey};
use message_hook::HotShotMessageHook;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use signature_key::WrappedSignatureKey;

/// Allows hooking of incoming messages to the CDN
pub mod message_hook;

/// The CDN's signature key implementation, which wraps the real signature key
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

/// Allows conversion from a `HotShotTopic` to a `Topic`
impl From<HotShotTopic> for Topic {
    fn from(topic: HotShotTopic) -> Self {
        match topic {
            HotShotTopic::Global => Topic::Global,
            HotShotTopic::Da => Topic::Da,
        }
    }
}

/// Implement the `TopicTrait` for our `Topic` enum. We need this to filter
/// topics that are not implemented at the application level.
impl TopicTrait for Topic {}

/// The production run definition for the Push CDN.
/// Uses the real protocols and a Redis discovery client.
pub struct ProductionDef<K: SignatureKey + 'static>(PhantomData<K>);
impl<K: SignatureKey + 'static> RunDef for ProductionDef<K> {
    type User = UserDef<K>;
    type Broker = BrokerDef<K>;
    type DiscoveryClientType = Redis;
    type Topic = Topic;
}

/// The user definition for the Push CDN.
/// Uses the TCP+TLS protocol and untrusted middleware.
pub struct UserDef<K: SignatureKey + 'static>(PhantomData<K>);
impl<K: SignatureKey + 'static> ConnectionDef for UserDef<K> {
    type Scheme = WrappedSignatureKey<K>;
    type Protocol = TcpTls;
    type MessageHook = HotShotMessageHook;
}

/// The broker definition for the Push CDN.
/// Uses the TCP protocol and trusted middleware.
pub struct BrokerDef<K: SignatureKey + 'static>(PhantomData<K>);
impl<K: SignatureKey> ConnectionDef for BrokerDef<K> {
    type Scheme = WrappedSignatureKey<K>;
    type Protocol = Tcp;
    type MessageHook = NoMessageHook;
}

/// The client definition for the Push CDN. Uses the TCP+TLS
/// protocol and no middleware. Differs from the user
/// definition in that is on the client-side.
#[derive(Clone)]
pub struct ClientDef<K: SignatureKey + 'static>(PhantomData<K>);
impl<K: SignatureKey> ConnectionDef for ClientDef<K> {
    type Scheme = WrappedSignatureKey<K>;
    type Protocol = TcpTls;
    type MessageHook = NoMessageHook;
}

/// The testing run definition for the Push CDN.
/// Uses the real protocols, but with an embedded discovery client.
pub struct TestingDef<K: SignatureKey + 'static>(PhantomData<K>);
impl<K: SignatureKey + 'static> RunDef for TestingDef<K> {
    type User = UserDef<K>;
    type Broker = BrokerDef<K>;
    type DiscoveryClientType = Embedded;
    type Topic = Topic;
}
