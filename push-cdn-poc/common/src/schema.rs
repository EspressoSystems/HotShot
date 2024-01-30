use std::net::SocketAddr;

use crate::{
    err,
    error::{BrokerError, Result},
    topic::Topic,
};
use rkyv::{AlignedVec, Archive, Deserialize, Serialize};

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[archive(check_bytes)]
pub enum Message {
    Authenticate(Authenticate),
    AuthenticateResponse(AuthenticateResponse),

    Identity(SocketAddr),

    Subscribe(Subscribe),
    Unsubscribe(Unsubscribe),

    Broadcast(Vec<Topic>, Vec<u8>),
    Direct(Vec<u8>, Vec<u8>),
    Data(Vec<u8>),
}

impl Message {
    pub fn serialize(&self) -> Result<AlignedVec> {
        err!(
            match self {
                // note: 160 chosen arbitrarily. tune to individual expected message size
                Message::Authenticate(_) => rkyv::to_bytes::<_, 160>(self),
                Message::AuthenticateResponse(_) => rkyv::to_bytes::<_, 160>(self),
                Message::Subscribe(_) => rkyv::to_bytes::<_, 16>(self),
                Message::Unsubscribe(_) => rkyv::to_bytes::<_, 16>(self),
                Message::Broadcast(_, _) => rkyv::to_bytes::<_, 160>(self),
                Message::Direct(_, _) => rkyv::to_bytes::<_, 160>(self),
                Message::Data(_) => rkyv::to_bytes::<_, 160>(self),
                Message::Identity(_) => rkyv::to_bytes::<_, 16>(self),
            },
            SerializeError,
            "failed to serialize message"
        )
    }

    pub fn deserialize(bytes: &[u8]) -> Result<Self> {
        let archived = err!(
            rkyv::check_archived_root::<Message>(bytes),
            DeserializeError,
            "failed to validate message"
        )?;
        err!(
            archived.deserialize(&mut rkyv::Infallible),
            DeserializeError,
            "failed to deserialize message"
        )
    }
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[archive(check_bytes)]
pub struct Authenticate {
    pub verify_key: Vec<u8>,
    pub timestamp: i64,
    pub signature: Vec<u8>,
    pub initial_topics: Vec<Topic>,
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[archive(check_bytes)]
pub struct Subscribe {
    pub topics: Vec<Topic>,
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[archive(check_bytes)]
pub struct Unsubscribe {
    pub topics: Vec<Topic>,
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[archive(check_bytes)]
pub struct AuthenticateResponse {
    pub success: bool,
    pub reason: String,
}

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq)]
#[archive(check_bytes)]
pub struct Dummy(pub u64);
