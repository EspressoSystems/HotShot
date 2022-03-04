use crate::network_node::NetworkNodeType;
use libp2p::{identity::Keypair, Multiaddr};
use serde::{
    de::{Unexpected, Visitor},
    ser::Error,
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::fmt;

/// starting network topology
#[derive(Serialize, Deserialize, Clone)]
pub struct NodeDescription {
    /// keypair for a node
    #[serde(deserialize_with = "deserialize_keypair")]
    #[serde(serialize_with = "serialize_keypair")]
    pub identity: Keypair,
    /// external multiaddr the node is running on
    pub multiaddr: Multiaddr,
    /// internal address to bind to
    pub bound_addr: Multiaddr,
    /// the type of node
    pub node_type: NetworkNodeType,
}

/// deserialize a keypair
/// # Errors
/// todo...
pub fn deserialize_keypair<'de, D>(deserializer: D) -> Result<Keypair, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_seq(KeypairVisitor)
}
struct KeypairVisitor;

impl<'de> Visitor<'de> for KeypairVisitor {
    type Value = Keypair;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a keypair deserialized in protobuf format")
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut parsed: Vec<u8> = Vec::new();
        while let Ok(Some(a)) = seq.next_element() {
            parsed.push(a);
        }
        // we couldn't deserialize the keypair, so the value
        // is unexpected
        Keypair::from_protobuf_encoding(&parsed)
            .map_err(|_e| serde::de::Error::invalid_value(Unexpected::Seq, &self))
    }
}

/// serialize a keypair
fn serialize_keypair<S>(x: &Keypair, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let bytes = Keypair::to_protobuf_encoding(x)
        .map_err(|_e| S::Error::custom("failed to encode keypair"))?;
    s.serialize_bytes(&bytes)
}
