use std::hash::Hasher;
use std::marker::PhantomData;
use std::num::NonZeroUsize;

use anyhow::{Context, Result};
use cdn_broker::reexports::def::hook::{HookResult, MessageHookDef};
use cdn_broker::reexports::message::{Broadcast, Direct, Message as PushCdnMessage};
use hotshot_types::traits::node_implementation::NodeType;
use lru::LruCache;
use twox_hash::xxh3::Hash64;

#[derive(Clone)]
/// The message hook for `HotShot` messages. Each user has a unique message hook.
pub struct HotShotMessageHook<T: NodeType> {
    /// The cache for message hashes. We use this to deduplicate a sliding window of
    /// 100 messages.
    message_hash_cache: LruCache<u64, ()>,

    /// The phantom data for the node type
    pd: PhantomData<T>,
}

impl<T: NodeType> HotShotMessageHook<T> {
    /// Create a new `HotShotMessageHook`
    pub fn new() -> Self {
        Self {
            message_hash_cache: LruCache::new(NonZeroUsize::new(100).unwrap()),
            pd: PhantomData,
        }
    }

    /// Process incoming broadcast messages from the user
    fn process_broadcast_message(&mut self, broadcast: &mut Broadcast) -> Result<HookResult> {
        // Calculate the hash of the message
        let mut hasher = Hash64::default();
        hasher.write(&broadcast.message);
        for topic in &broadcast.topics {
            hasher.write(&[*topic]);
        }

        // Make sure we have not already seen it
        if self.message_hash_cache.put(hasher.finish(), ()).is_some() {
            return Ok(HookResult::SkipMessage);
        }

        // TODO: Deserialize the message
        // let message = Self::deserialize_message(&direct.message)?;

        Ok(HookResult::ProcessMessage)
    }

    fn process_direct_message(&mut self, direct: &mut Direct) -> Result<HookResult> {
        // Calculate the hash of the message
        let mut hasher = Hash64::default();
        hasher.write(&direct.message);
        hasher.write(&direct.recipient);

        // Make sure we have not already seen it
        if self.message_hash_cache.put(hasher.finish(), ()).is_some() {
            return Ok(HookResult::SkipMessage);
        }

        // TODO: Deserialize the message
        // let message = Self::deserialize_message(&direct.message)?;

        Ok(HookResult::ProcessMessage)
    }

    // TODO
    // fn deserialize_message(message: &[u8]) -> Result<Message<T>> {
    //     // Hack off the version
    //     let (_, message) =
    //         Version::deserialize(&message).with_context(|| "failed to deserialize message")?;

    //     // Deserialize the message
    //     Serializer::<StaticVersion<0, 1>>::deserialize_no_version(&message)
    //         .with_context(|| "failed to deserialize message")
    // }
}

impl<T: NodeType> MessageHookDef for HotShotMessageHook<T> {
    /// Implement the hook trait for `HotShotMessageHook`
    fn on_message_received(&mut self, message: &mut PushCdnMessage) -> Result<HookResult> {
        match message {
            PushCdnMessage::Broadcast(broadcast) => self
                .process_broadcast_message(broadcast)
                .with_context(|| "failed to process broadcast message"),

            PushCdnMessage::Direct(direct) => self
                .process_direct_message(direct)
                .with_context(|| "failed to process direct message"),

            _ => Ok(HookResult::ProcessMessage),
        }
    }
}
