#![allow(clippy::unnecessary_wraps)]

use std::hash::Hasher;
use std::num::NonZeroUsize;

use anyhow::{Context, Result};
use cdn_broker::reexports::def::hook::{HookResult, MessageHookDef};
use cdn_broker::reexports::message::{Broadcast, Direct, Message as PushCdnMessage};
use lru::LruCache;
use twox_hash::xxh3::Hash64;

#[derive(Clone)]
/// The message hook for `HotShot` messages. Each user has a unique message hook.
pub struct HotShotMessageHook {
    /// The cache for message hashes. We use this to deduplicate a sliding window of
    /// 100 messages.
    message_hash_cache: LruCache<u64, ()>,
}

impl Default for HotShotMessageHook {
    /// # Panics
    /// If 100 < 0
    fn default() -> Self {
        Self {
            message_hash_cache: LruCache::new(NonZeroUsize::new(100).unwrap()),
        }
    }
}

impl HotShotMessageHook {
    /// Create a new `HotShotMessageHook`
    ///
    /// # Panics
    /// If 100 < 0
    #[must_use]
    pub fn new() -> Self {
        Self {
            message_hash_cache: LruCache::new(NonZeroUsize::new(100).unwrap()),
        }
    }

    /// Process against the local message cache. This is used to deduplicate messages.
    /// Returns `true` if the message has been seen before, `false` otherwise.
    ///
    /// - `auxiliary_data` is used to take into account the message recipient or topics associated.
    fn message_already_seen(&mut self, message: &[u8], auxiliary_data: &[u8]) -> bool {
        // Calculate the hash of the message
        let mut hasher = Hash64::default();
        hasher.write(message);
        hasher.write(auxiliary_data);

        // Add it, returning if we've seen it before
        self.message_hash_cache.put(hasher.finish(), ()).is_some()
    }

    /// Process incoming broadcast messages from the user
    fn process_broadcast_message(&mut self, broadcast: &mut Broadcast) -> Result<HookResult> {
        // Skip the message if we've already seen it
        if self.message_already_seen(&broadcast.message, &broadcast.topics) {
            return Ok(HookResult::SkipMessage);
        }

        Ok(HookResult::ProcessMessage)
    }

    /// Process incoming direct messages from the user
    fn process_direct_message(&mut self, direct: &mut Direct) -> Result<HookResult> {
        // Skip the message if we've already seen it
        if self.message_already_seen(&direct.message, &direct.recipient) {
            return Ok(HookResult::SkipMessage);
        }

        Ok(HookResult::ProcessMessage)
    }
}

impl MessageHookDef for HotShotMessageHook {
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    pub fn deduplication_broadcast() {
        // Create a new message hook
        let mut hook = HotShotMessageHook::default();

        // Create a message
        let mut message = Vec::new();
        message.extend_from_slice(b"Hello, world!");

        // Create a broadcast message
        let mut broadcast = Broadcast {
            message,
            topics: vec![],
        };

        // Process the message, make sure it would've been sent
        let result = hook.process_broadcast_message(&mut broadcast);
        assert!(
            result.is_ok() && result.unwrap() == HookResult::ProcessMessage,
            "Message should have been processed but was not"
        );

        // Send it again, this time it should be skipped
        let result = hook.process_broadcast_message(&mut broadcast);
        assert!(
            result.is_ok() && result.unwrap() == HookResult::SkipMessage,
            "Message should have been skipped but was not"
        );

        // Alter the topics, it should be processed
        broadcast.topics.push(1);
        let result = hook.process_broadcast_message(&mut broadcast);
        assert!(
            result.is_ok() && result.unwrap() == HookResult::ProcessMessage,
            "Same message with different topics should have been processed but was not"
        );

        // Alter the message, it should be processed
        broadcast.message.extend_from_slice(b"!");
        broadcast.topics.clear();
        let result = hook.process_broadcast_message(&mut broadcast);
        assert!(
            result.is_ok() && result.unwrap() == HookResult::ProcessMessage,
            "Different message with same topics should have been processed but was not"
        );
    }

    #[test]
    pub fn deduplication_direct() {
        // Create a new message hook
        let mut hook = HotShotMessageHook::default();

        // Create a message
        let mut message = Vec::new();
        message.extend_from_slice(b"Hello, world!");

        // Create a broadcast message
        let mut direct = Direct {
            message,
            recipient: vec![],
        };

        // Process the message, make sure it would've been sent
        let result = hook.process_direct_message(&mut direct);
        assert!(
            result.is_ok() && result.unwrap() == HookResult::ProcessMessage,
            "Message should have been processed but was not"
        );

        // Send it again, this time it should be skipped
        let result = hook.process_direct_message(&mut direct);
        assert!(
            result.is_ok() && result.unwrap() == HookResult::SkipMessage,
            "Message should have been skipped but was not"
        );

        // Alter the topics, it should be processed
        direct.recipient.push(1);
        let result = hook.process_direct_message(&mut direct);
        assert!(
            result.is_ok() && result.unwrap() == HookResult::ProcessMessage,
            "Same message with different recipient should have been processed but was not"
        );

        // Alter the message, it should be processed
        direct.message.extend_from_slice(b"!");
        direct.recipient.clear();
        let result = hook.process_direct_message(&mut direct);
        assert!(
            result.is_ok() && result.unwrap() == HookResult::ProcessMessage,
            "Different message with same recipient should have been processed but was not"
        );
    }
}
