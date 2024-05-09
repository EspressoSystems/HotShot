//! The rewindable trait gives a node the ability to replay its message history.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Serialize};

/// [`Rewindable`] is a trait which gives the implementor the ability to
/// store and write out its state for the purposes of later debugging.
pub trait Rewindable {
    /// The message type being stored.
    type Message: Clone + std::fmt::Debug + Serialize + DeserializeOwned;

    /// Stores a message into the internal state.
    fn store_message(&mut self, timestamp: DateTime<Utc>, message: Self::Message) -> Result<()>;

    /// Writes the internal state messages out to a location for later processing.
    fn write_stored_messages(&mut self) -> Result<()>;
}
