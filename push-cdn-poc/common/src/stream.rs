use derive_more::Deref;
use std::{
    hash::{Hash, Hasher},
    sync::Arc,
};

use crate::{
    constant::STREAM_DATA_LIMIT,
    err,
    error::{BrokerError, Result},
    schema::Message,
};

pub struct RecvStream(pub quinn::RecvStream);

pub struct SendStream(pub quinn::SendStream);

impl RecvStream {
    pub async fn recv_message(mut self) -> Result<Message> {
        // read message
        let message_bytes = err!(
            self.0.read_to_end(STREAM_DATA_LIMIT).await,
            ReadError,
            "failed to read from stream"
        )?;

        // deserialize message
        let message = err!(
            Message::deserialize(&message_bytes,),
            DeserializeError,
            "failed to deserialize message"
        )?;

        Ok(message)
    }
}

impl SendStream {
    pub async fn send_message(mut self, message: &Arc<Message>) -> Result<()> {
        // serialize message
        let serialized_message = message.serialize()?;

        // write message
        err!(
            self.0.write_all(&serialized_message).await,
            WriteError,
            "failed to write to stream"
        )?;

        // close connection, framing the message
        err!(self.0.finish().await, StreamError, "failed to close stream")?;

        Ok(())
    }
}

#[derive(Clone, Deref)]
pub struct Connection(pub quinn::Connection);

impl PartialEq for Connection {
    fn eq(&self, other: &Self) -> bool {
        self.0.stable_id() == other.0.stable_id()
    }
}

impl Eq for Connection {
    fn assert_receiver_is_total_eq(&self) {}
}

impl Hash for Connection {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.stable_id().hash(state);
    }
}

impl Connection {
    pub async fn send_request(&self, message: &Arc<Message>) -> Result<Message> {
        // open stream
        let (send_stream, recv_stream) = err!(
            self.0.open_bi().await,
            StreamError,
            "failed to open bidirectional stream"
        )?;

        // send message
        SendStream(send_stream).send_message(message).await?;

        // await response
        RecvStream(recv_stream).recv_message().await
    }

    pub async fn recv_request(&self) -> Result<(Message, SendStream)> {
        // accept stream
        let (send_stream, recv_stream) = err!(
            self.0.accept_bi().await,
            StreamError,
            "failed to accept bidirectional stream"
        )?;

        // await response
        Ok((
            RecvStream(recv_stream).recv_message().await?,
            SendStream(send_stream),
        ))
    }

    pub async fn send_message(&self, message: &Arc<Message>) -> Result<()> {
        // open stream
        let stream = err!(
            self.0.open_uni().await,
            StreamError,
            "failed to open unidirectional stream"
        )?;

        // write message
        SendStream(stream).send_message(message).await?;

        Ok(())
    }

    pub async fn recv_message(&self) -> Result<Message> {
        // accept stream
        let stream = err!(
            self.0.accept_uni().await,
            StreamError,
            "failed to accept unidirectional stream"
        )?;

        // write message
        RecvStream(stream).recv_message().await
    }
}
