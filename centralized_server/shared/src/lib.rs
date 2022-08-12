use bincode::config::*;
use hotshot_types::traits::signature_key::SignatureKey;

/// For the wire format, we use bincode with the following options:
///   - No upper size limit
///   - Litte endian encoding
///   - Varint encoding
///   - Reject trailing bytes
#[allow(clippy::type_complexity)]
pub fn bincode_opts() -> WithOtherTrailing<
    WithOtherIntEncoding<
        WithOtherEndian<WithOtherLimit<DefaultOptions, bincode::config::Infinite>, LittleEndian>,
        VarintEncoding,
    >,
    RejectTrailing,
> {
    bincode::DefaultOptions::new()
        .with_no_limit()
        .with_little_endian()
        .with_varint_encoding()
        .reject_trailing_bytes()
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug)]
#[serde(bound(deserialize = ""))]
pub enum ToServer<K: SignatureKey> {
    Identify { key: K },
    Broadcast { message: Vec<u8> },
    Direct { target: K, message: Vec<u8> },
}

#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug)]
#[serde(bound(deserialize = ""))]
pub enum FromServer<K: SignatureKey> {
    NodeConnected { key: K },
    NodeDisconnected { key: K },
    Broadcast { message: Vec<u8> },
    Direct { message: Vec<u8> },
}
