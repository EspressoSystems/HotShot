use anyhow::{Context, Result};

use super::{request::Request, Serializable};

/// The message type for the request-response protocol
pub enum Message<R: Request> {
    /// A request message
    Request(R),
    /// A response message
    Response(R::Response),
}

/// A blanket implementation of the [`Serializable`] trait for any [`Message`]
impl<R: Request> Serializable for Message<R> {
    /// Converts any [`Message`] to bytes
    fn to_bytes(&self) -> Result<Vec<u8>> {
        // Convert the message to bytes based on the type. By default it is just type-prefixed
        let (type_byte, content_bytes) = match self {
            Message::Request(request) => (
                0,
                request
                    .to_bytes()
                    .with_context(|| "failed to serialize request")?,
            ),
            Message::Response(response) => (
                1,
                response
                    .to_bytes()
                    .with_context(|| "failed to serialize response")?,
            ),
        };

        // Combine the type byte and content bytes
        let bytes = [&[type_byte], content_bytes.as_slice()].concat();

        Ok(bytes)
    }

    /// Convert bytes to a [`Message`]
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // Get the message type
        let type_byte = bytes
            .get(0)
            .ok_or(anyhow::anyhow!("message type not found"))?;

        // Deserialize the message based on the type
        match type_byte {
            0 => {
                let request_bytes = bytes
                    .get(1..)
                    .ok_or(anyhow::anyhow!("request bytes not found"))?;
                Ok(Message::Request(R::from_bytes(request_bytes)?))
            }
            1 => {
                let response_bytes = bytes
                    .get(1..)
                    .ok_or(anyhow::anyhow!("response bytes not found"))?;
                Ok(Message::Response(R::Response::from_bytes(response_bytes)?))
            }
            _ => Err(anyhow::anyhow!("invalid message type")),
        }
    }
}
