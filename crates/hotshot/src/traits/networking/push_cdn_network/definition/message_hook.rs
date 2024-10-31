#![allow(clippy::unnecessary_wraps)]

use std::hash::Hasher;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::u32;

use anyhow::{Context, Result};
use cdn_broker::reexports::def::hook::{HookResult, MessageHookDef};
use cdn_broker::reexports::message::{Broadcast, Direct, Message as PushCdnMessage};
use gcr::{Gcr, GcrRequestError};
use lru::LruCache;
use parking_lot::Mutex;
use tracing::warn;
use twox_hash::xxh3::Hash64;

/// The type of message being processed. Is used downstream to determine
/// which sample and average to use when processing a message.
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
enum MessageType {
    /// A broadcast message
    Broadcast,
    /// A direct message
    Direct,
}

/// An average representing the number of bytes processed per second
struct Average {
    /// The number of bytes processed
    num_bytes: f64,

    /// The number of samples
    num_samples: u64,
}

impl Average {
    /// Create a new `Average` instance
    fn new() -> Self {
        Self {
            num_bytes: 0.0,
            num_samples: 0,
        }
    }

    /// Add a sample to the average
    fn add(&mut self, num: f64) {
        // Add the bytes and increment the number of samples
        self.num_bytes = self.num_bytes + num;
        self.num_samples = self.num_samples.saturating_add(1);
    }

    /// Finalize the average, resetting the internal state and returning the new average
    fn commit(&mut self) -> f64 {
        // Calculate the new average
        let value = self.num_bytes / self.num_samples as f64;

        // Reset the internal state
        self.num_bytes = 0.0;
        self.num_samples = 0;

        // Return the new average
        value
    }
}

/// A wrapper around an average that caches the last calculated value
#[derive(Clone)]
struct CachedAverage {
    /// The inner average
    average: Arc<Mutex<Average>>,

    /// The last calculated value
    last_calculated: Arc<AtomicU32>,
}

impl CachedAverage {
    /// Create a new `CachedAverage` instance
    fn new() -> Self {
        Self {
            average: Arc::new(Mutex::new(Average::new())),
            last_calculated: Arc::new(AtomicU32::new(0)),
        }
    }
}

/// A sample representing the number of bytes processed per second
#[derive(Clone)]
pub struct Sample {
    /// The number of bytes processed
    num_bytes: f64,

    /// The time of the last commit
    last_committed_time: Instant,
}

impl Sample {
    /// Create a new `Sample` instance
    fn new() -> Self {
        Self {
            num_bytes: 0.0,
            last_committed_time: Instant::now(),
        }
    }

    /// Add a number of bytes to the sample
    fn add(&mut self, num: f64) {
        self.num_bytes += num;
    }

    /// Commit the sample, returning the number of bytes processed per second and
    /// resetting the internal state
    fn commit(&mut self) -> f64 {
        let elapsed = self.last_committed_time.elapsed();
        self.last_committed_time = Instant::now();
        let value = self.num_bytes / elapsed.as_secs() as f64;
        self.num_bytes = 0.0;
        value
    }
}

/// The message hook for `HotShot` messages. Each user has a unique
#[derive(Clone)]
pub struct HotShotMessageHook {
    /// The cache for message hashes. We use this to deduplicate a sliding window of
    /// 100 messages.
    message_hash_cache: LruCache<u64, ()>,

    /// The reference counter so we can keep track of the number of hooks there are
    num_hooks: Arc<()>,

    /// The unique identifier for the hook. This is used to uniquely identify the hook
    /// by the consumer
    identifier: u64,

    /// The cache of previously dropped GCRs by identifier. This is used to prevent a user
    /// from reconnecting and using a brand new GCR instance (empty rate limit)
    dropped_gcr_cache: Arc<Mutex<LruCache<u64, (Gcr, Gcr)>>>,

    /// The global average number of bytes per second for direct messages
    global_average_direct_bps: CachedAverage,

    /// The global average number of bytes per second for broadcast messages
    global_average_broadcast_bps: CachedAverage,

    /// The local GCR instance for direct messages
    local_gcr_direct: Gcr,

    /// The local GCR instance for broadcast messages
    local_gcr_broadcast: Gcr,

    /// The local, running number of consumed bytes for direct messages
    local_sample_direct_bps: Sample,

    /// The local, running number of consumed bytes for broadcast messages
    local_sample_broadcast_bps: Sample,
}

impl Default for HotShotMessageHook {
    /// # Panics
    /// If 100 < 0
    fn default() -> Self {
        Self {
            message_hash_cache: LruCache::new(NonZeroUsize::new(100).unwrap()),
            num_hooks: Arc::new(()),
            identifier: rand::random(),
            dropped_gcr_cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(1000).unwrap(),
            ))),
            local_gcr_direct: Gcr::new(u32::MAX, Duration::from_secs(60), Some(u32::MAX)).unwrap(),
            local_gcr_broadcast: Gcr::new(u32::MAX, Duration::from_secs(60), Some(u32::MAX))
                .unwrap(),
            local_sample_direct_bps: Sample::new(),
            local_sample_broadcast_bps: Sample::new(),
            global_average_direct_bps: CachedAverage::new(),
            global_average_broadcast_bps: CachedAverage::new(),
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
        Self::default()
    }

    /// Process a message against the moving average
    /// Returns whether or not the message should be skipped.
    fn process_against_average(
        &mut self,
        message_len: usize,
        message_type: MessageType,
    ) -> HookResult {
        // Get the correct sample, `GCR`, and global average based on the message type
        let (local_sample, gcr, global_average) = match message_type {
            MessageType::Broadcast => (
                &mut self.local_sample_broadcast_bps,
                &mut self.local_gcr_broadcast,
                &mut self.global_average_broadcast_bps,
            ),
            MessageType::Direct => (
                &mut self.local_sample_direct_bps,
                &mut self.local_gcr_direct,
                &mut self.global_average_direct_bps,
            ),
        };

        // Check against the `GCR` instance, skipping if we're rate limited
        // If we hit some other request error (like parameter error), we'll process
        // the message anyway
        if let Err(err) = gcr.request(message_len as u32) {
            if let GcrRequestError::DeniedFor(_) = err {
                return HookResult::SkipMessage;
            }

            warn!("Failed to check GCR instance: {err}");
        }

        // Add the message to our local sample
        local_sample.add(message_len as f64);

        // If it's been a minute,
        if local_sample.last_committed_time.elapsed() > Duration::from_secs(60) {
            // Commit the sample to the global average
            let value = local_sample.commit();

            // Store the new average
            let mut global_average_guard = global_average.average.lock();
            global_average_guard.add(value);

            // If we've collected enough samples, commit the average
            if global_average_guard.num_samples >= Arc::strong_count(&self.num_hooks) as u64 {
                // Commit the average
                let new_average = global_average_guard.commit();

                // Update the last calculated value
                global_average
                    .last_calculated
                    .store(new_average as u32, Ordering::Relaxed);
            }
            drop(global_average_guard);

            // Update the `Gcr` instance to be double the last calculated average
            let new_global_average = global_average.last_calculated.load(Ordering::Relaxed);
            if let Err(e) = gcr.adjust(
                new_global_average.saturating_mul(2),
                Duration::from_secs(60),
                Some(new_global_average.saturating_mul(4)),
            ) {
                warn!("Failed to adjust GCR instance: {e}");
            }
        }

        HookResult::ProcessMessage
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

        // Check against the average message rate
        if self.process_against_average(broadcast.message.len(), MessageType::Broadcast)
            != HookResult::ProcessMessage
        {
            warn!("Broadcast message not processed due to high message rate");
            return Ok(HookResult::SkipMessage);
        };

        Ok(HookResult::ProcessMessage)
    }

    /// Process incoming direct messages from the user
    fn process_direct_message(&mut self, direct: &mut Direct) -> Result<HookResult> {
        // Skip the message if we've already seen it
        if self.message_already_seen(&direct.message, &direct.recipient) {
            return Ok(HookResult::SkipMessage);
        }

        // Check against the average message rate
        if self.process_against_average(direct.message.len(), MessageType::Direct)
            != HookResult::ProcessMessage
        {
            warn!("Direct message not processed due to high message rate");
            return Ok(HookResult::SkipMessage);
        };

        Ok(HookResult::ProcessMessage)
    }
}

/// Implement the hook trait for `HotShotMessageHook`
impl MessageHookDef for HotShotMessageHook {
    /// Handle a received message
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

    /// Setting the identifier for the hook should update the GCR instances if we have
    /// previously dropped ones for it
    fn set_identifier(&mut self, identifier: u64) {
        // Set the identifier
        self.identifier = identifier;

        // If we have a GCR instance for it, use that instead of creating a new one
        if let Some((direct, broadcast)) = self.dropped_gcr_cache.lock().get(&identifier) {
            self.local_gcr_direct = direct.clone();
            self.local_gcr_broadcast = broadcast.clone();
        }
    }
}

impl Drop for HotShotMessageHook {
    /// When the hook is dropped, we should cache the GCR instances in case the user reconnects
    fn drop(&mut self) {
        // If there is already a GCR instance for the identifier, use the one with the least capacity
        let mut dropped_gcr_cache_guard = self.dropped_gcr_cache.lock();

        if let Some((previous_direct, previous_broadcast)) =
            dropped_gcr_cache_guard.pop(&self.identifier)
        {
            // Set the local GCR instances to the ones with the least capacity
            if previous_direct.capacity() < self.local_gcr_direct.capacity() {
                self.local_gcr_direct = previous_direct;
            }

            // Do the same for the broadcast GCR instance
            if previous_broadcast.capacity() < self.local_gcr_broadcast.capacity() {
                self.local_gcr_broadcast = previous_broadcast;
            }
        }

        // Cache the GCR instances
        dropped_gcr_cache_guard.put(
            self.identifier,
            (
                self.local_gcr_direct.clone(),
                self.local_gcr_broadcast.clone(),
            ),
        );
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
