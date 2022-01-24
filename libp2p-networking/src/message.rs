use std::{
    io::{Error, ErrorKind},
    marker::PhantomData,
};

use async_std::io;
use async_trait::async_trait;
use bincode::{DefaultOptions, Options};
use futures::{AsyncRead, AsyncWrite, AsyncWriteExt};
use libp2p::{
    core::{
        upgrade::{read_length_prefixed, write_length_prefixed},
        ProtocolName,
    },
    gossipsub::GossipsubMessage,
    request_response::RequestResponseCodec,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::GossipMsg;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Message {
    pub sender: String,
    pub content: String,
    pub topic: String,
}

impl GossipMsg for Message {
    fn topic(&self) -> libp2p::gossipsub::IdentTopic {
        libp2p::gossipsub::IdentTopic::new(self.topic.clone())
    }
    fn data(&self) -> Vec<u8> {
        self.content.as_bytes().into()
    }
}

impl From<GossipsubMessage> for Message {
    fn from(msg: GossipsubMessage) -> Self {
        let content = String::from_utf8_lossy(&msg.data).to_string();
        let sender = msg
            .source
            .map_or_else(|| "UNKNOWN".to_string(), |p| p.to_string());
        Message {
            sender,
            content,
            topic: msg.topic.into_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DirectMessageProtocol();
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DirectMessageCodec<T: Send>(pub PhantomData<T>);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectMessageRequest<T: Send>(pub T);
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectMessageResponse();

impl ProtocolName for DirectMessageProtocol {
    fn protocol_name(&self) -> &[u8] {
        "/spectrum_send_msg/1".as_bytes()
    }
}

// TODO are generics useful here? Could also just pass in vec of already serialized bytes
#[async_trait]
impl<M: Send + Sync + std::fmt::Debug + Serialize + DeserializeOwned> RequestResponseCodec
    for DirectMessageCodec<M>
{
    type Protocol = DirectMessageProtocol;

    type Request = DirectMessageRequest<M>;

    type Response = DirectMessageResponse;

    async fn read_request<T>(
        &mut self,
        _: &DirectMessageProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        // FIXME magic numbers...
        // it looks like the easiest thing to do
        // is to set an upper limit threshold and use that
        let vec = read_length_prefixed(io, 1_000_000).await?;

        let bincode_options = DefaultOptions::new().with_limit(16_384);

        if vec.is_empty() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        // NOTE error handling could be better...
        // but the trait locks into io::Result : (
        Ok(DirectMessageRequest(
            bincode_options
                .deserialize(&vec)
                .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?,
        ))
    }

    async fn read_response<T>(
        &mut self,
        _: &DirectMessageProtocol,
        _: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        // NOTE no replies
        Ok(DirectMessageResponse())
    }

    async fn write_request<T>(
        &mut self,
        _: &DirectMessageProtocol,
        io: &mut T,
        DirectMessageRequest(msg): DirectMessageRequest<M>,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let bincode_options = DefaultOptions::new().with_limit(16_384);
        write_length_prefixed(
            io,
            bincode_options
                .serialize(&msg)
                .map_err(|e| Error::new(io::ErrorKind::Other, e.to_string()))?,
        )
        .await?;
        io.close().await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &DirectMessageProtocol,
        io: &mut T,
        DirectMessageResponse(): DirectMessageResponse,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        io.close().await?;
        Ok(())
    }
}
