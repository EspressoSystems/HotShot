use bincode::config::*;
use hotshot_types::{
    message::Message,
    traits::{signature_key::SignatureKey, BlockContents, State, Transaction},
};

/// For the wire format, we use bincode with the following options:
///   - No upper size limit
///   - Litte endian encoding
///   - Varint encoding
///   - Reject trailing bytes
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

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(bound(deserialize = ""))]
pub enum ToServer<
    B: BlockContents<N>,
    T: Transaction<N>,
    S: State<N>,
    K: SignatureKey,
    const N: usize,
> {
    Identify {
        key: K,
    },
    Broadcast {
        message: Message<B, T, S, K, N>,
    },
    Direct {
        target: K,
        message: Message<B, T, S, K, N>,
    },
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(bound(deserialize = ""))]
pub enum FromServer<
    B: BlockContents<N>,
    T: Transaction<N>,
    S: State<N>,
    K: SignatureKey,
    const N: usize,
> {
    NodeConnected { key: K },
    NodeDisconnected { key: K },
    Broadcast { message: Message<B, T, S, K, N> },
    Direct { message: Message<B, T, S, K, N> },
}
