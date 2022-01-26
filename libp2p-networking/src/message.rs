use async_std::io;
use async_trait::async_trait;

use futures::{AsyncRead, AsyncWrite, AsyncWriteExt};
use libp2p::{
    core::{
        upgrade::{read_length_prefixed, write_length_prefixed},
        ProtocolName,
    },
    gossipsub::GossipsubMessage,
    request_response::RequestResponseCodec,
};
use serde::{Deserialize, Serialize};

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
pub struct DirectMessageCodec();
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectMessageRequest(pub Vec<u8>);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectMessageResponse(pub Vec<u8>);

impl ProtocolName for DirectMessageProtocol {
    fn protocol_name(&self) -> &[u8] {
        "/spectrum_send_msg/1".as_bytes()
    }
}

#[async_trait]
impl RequestResponseCodec for DirectMessageCodec {
    type Protocol = DirectMessageProtocol;

    type Request = DirectMessageRequest;

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
        let msg = read_length_prefixed(io, 1_000_000).await?;

        // NOTE we don't error here. We'll wrap this in a behaviour and get better error messages
        // there
        Ok(DirectMessageRequest(msg))
    }

    async fn read_response<T>(
        &mut self,
        _: &DirectMessageProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let msg = read_length_prefixed(io, 1_000_000).await?;
        Ok(DirectMessageResponse(msg))
    }

    async fn write_request<T>(
        &mut self,
        _: &DirectMessageProtocol,
        io: &mut T,
        DirectMessageRequest(msg): DirectMessageRequest,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_length_prefixed(io, msg).await?;
        io.close().await?;

        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &DirectMessageProtocol,
        io: &mut T,
        DirectMessageResponse(msg): DirectMessageResponse,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_length_prefixed(io, msg).await?;
        io.close().await?;
        Ok(())
    }
}
