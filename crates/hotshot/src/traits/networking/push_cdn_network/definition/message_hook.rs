#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]

use std::cmp;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use cdn_broker::reexports::def::hook::{HookResult, MessageHookDef};
use cdn_broker::reexports::message::{Broadcast, Direct, Message as PushCdnMessage};
use gcr::{Gcr, GcrRequestError};
use lru::LruCache;
use parking_lot::Mutex;
use tracing::warn;

/// The type of message being processed. Is used downstream to determine
/// which sample and average to use when processing a message.
#[derive(Eq, PartialEq, Clone, Copy, Debug)]
enum MessageType {
    /// A broadcast message
    Broadcast,
    /// A direct message
    Direct,
}

/// An average representing the number of bytes processed per period
struct Average {
    /// The number of bytes processed
    num_bytes: Mutex<f64>,

    /// The number of samples
    num_samples: Mutex<u64>,

    /// The last committed value
    #[allow(clippy::struct_field_names)]
    last_committed_average: AtomicU32,
}

impl Average {
    /// Create a new `Average` instance
    fn new() -> Self {
        Self {
            num_bytes: Mutex::new(0.0),
            num_samples: Mutex::new(0),
            last_committed_average: AtomicU32::new(0),
        }
    }

    /// Add a sample to the average
    fn add(&self, num: f64) {
        let mut num_bytes = self.num_bytes.lock();
        *num_bytes += num;

        let mut num_samples = self.num_samples.lock();
        *num_samples += 1;
    }

    /// Get the last committed average
    fn get_last_committed_average(&self) -> u32 {
        self.last_committed_average.load(Ordering::Relaxed)
    }

    /// Finalize the average, resetting the internal state and caching the new average.
    /// Returns `true` if we committed a new average, `false` otherwise.
    fn commit_if_necessary(&self, num_required_samples: usize) -> bool {
        let mut num_samples = self.num_samples.lock();

        // Return early if we don't have enough samples
        if *num_samples < num_required_samples as u64 {
            return false;
        }

        // Calculate the new average
        let mut num_bytes = self.num_bytes.lock();
        let new_average = (*num_bytes / *num_samples as f64) as u32;

        // Reset the internal state
        *num_bytes = 0.0;
        *num_samples = 0;

        // Cache the new average
        self.last_committed_average
            .store(new_average, Ordering::Relaxed);

        // Return the new average
        true
    }
}

/// A sample representing the number of bytes processed per period
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

    /// Commit the sample if necessary, returning the number of bytes processed per commit interval
    /// and resetting the internal state if we did.
    fn commit_if_necessary(&mut self, commit_interval: Duration) -> Option<f64> {
        // Get the elapsed time
        let elapsed = self.last_committed_time.elapsed();

        // Return early if we aren't past the commit interval
        if elapsed < commit_interval {
            return None;
        }

        // Calculate the new average
        let value = self.num_bytes / elapsed.div_duration_f64(commit_interval);

        // Reset the internal state
        self.last_committed_time = Instant::now();
        self.num_bytes = 0.0;

        // Return the new average
        Some(value)
    }
}

/// The message hook for `HotShot` messages. Each user has a unique
#[derive(Clone)]
pub struct HotShotMessageHook {
    // TODO: Potentially make use of this later
    /// The cache for message hashes. We use this to deduplicate a sliding window of
    /// 100 messages.
    // message_hash_cache: LruCache<u64, ()>,

    /// The reference counter so we can keep track of the number of hooks there are
    num_hooks: Arc<()>,

    /// The unique identifier for the hook. This is used to uniquely identify the hook
    /// by the consumer
    identifier: u64,

    /// The commit interval for a local sample -> global average
    commit_interval: Duration,

    /// The cache of previously dropped GCRs by identifier. This is used to prevent a user
    /// from reconnecting and using a brand new GCR instance (empty rate limit)
    dropped_gcr_cache: Arc<Mutex<LruCache<u64, (Gcr, Gcr)>>>,

    /// The global average number of bytes per period for direct messages
    global_average_direct_bps: Arc<Average>,

    /// The global average number of bytes per period for broadcast messages
    global_average_broadcast_bps: Arc<Average>,

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
        Self::new(Duration::from_secs(60), u32::MAX)
    }
}

impl HotShotMessageHook {
    /// Create a new `HotShotMessageHook`
    ///
    /// # Panics
    /// If 100 < 0
    #[must_use]
    pub fn new(period: Duration, starting_rate: u32) -> Self {
        Self {
            // message_hash_cache: LruCache::new(NonZeroUsize::new(100).unwrap()),
            num_hooks: Arc::new(()),
            identifier: rand::random(),
            dropped_gcr_cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(1000).unwrap(),
            ))),
            commit_interval: period,
            local_gcr_direct: Gcr::new(starting_rate, period, Some(u32::MAX)).unwrap(),
            local_gcr_broadcast: Gcr::new(starting_rate, period, Some(u32::MAX)).unwrap(),
            local_sample_direct_bps: Sample::new(),
            local_sample_broadcast_bps: Sample::new(),
            global_average_direct_bps: Arc::new(Average::new()),
            global_average_broadcast_bps: Arc::new(Average::new()),
        }
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
                &self.global_average_broadcast_bps,
            ),
            MessageType::Direct => (
                &mut self.local_sample_direct_bps,
                &mut self.local_gcr_direct,
                &self.global_average_direct_bps,
            ),
        };

        // Check against the `GCR` instance, skipping if we're rate limited
        // If we hit a parameter error, we'll process the message anyway
        if let Err(err) = gcr.request(message_len as u32) {
            if !matches!(err, GcrRequestError::ParametersOutOfRange(_)) {
                return HookResult::SkipMessage;
            }

            warn!("Failed to check GCR instance: {err}");
        }

        // Add the message to our local sample
        local_sample.add(message_len as f64);

        // Commit the local sample if necessary. If we did, update the global average
        // and adjust our GCR instance
        if let Some(local_average) = local_sample.commit_if_necessary(self.commit_interval) {
            // Add the local average to the global average
            global_average.add(local_average);

            // Commit the global average if we have enough samples
            global_average.commit_if_necessary(Arc::strong_count(&self.num_hooks) - 1);

            // Get the global average
            let last_committed_global_average = global_average.get_last_committed_average();

            // If the global average is greater than 0, update our GCR instance
            if last_committed_global_average > 0 {
                // Update our GCR instance such that:
                // - `rate` is 2 * the global average
                // - `max_burst` stays the same (the max u32)
                if let Err(e) = gcr.adjust(
                    // At the very least we want to allow 1000 bytes/period. This is fine because the max
                    // burst is quite high.
                    cmp::max(last_committed_global_average.saturating_mul(2), 1000),
                    self.commit_interval,
                    Some(u32::MAX),
                ) {
                    warn!("Failed to adjust GCR instance: {e}");
                }
            }
        }

        HookResult::ProcessMessage
    }

    // TODO: Potentially make use of this later
    // /// Process against the local message cache. This is used to deduplicate messages.
    // /// Returns `true` if the message has been seen before, `false` otherwise.
    // ///
    // /// - `auxiliary_data` is used to take into account the message recipient or topics associated.
    // fn message_already_seen(&mut self, message: &[u8], auxiliary_data: &[u8]) -> bool {
    //     // Calculate the hash of the message
    //     let mut hasher = Hash64::default();
    //     hasher.write(message);
    //     hasher.write(auxiliary_data);

    //     // Add it, returning if we've seen it before
    //     self.message_hash_cache.put(hasher.finish(), ()).is_some()
    // }

    /// Process incoming broadcast messages from the user
    fn process_broadcast_message(&mut self, broadcast: &mut Broadcast) -> Result<HookResult> {
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
    use std::thread::sleep;

    use super::*;

    #[test]
    fn test_hook_drop() {
        // Create a base hook
        let base_hook = HotShotMessageHook::new(Duration::from_millis(100), 100);

        // Clone the user hook
        let mut user_hook = base_hook.clone();
        user_hook.set_identifier(1);

        // Consume u32::MAX units of bandwidth
        user_hook.process_against_average(u32::MAX as usize, MessageType::Broadcast);

        // Drop the user hook
        drop(user_hook);

        // Create a new hook with the same identifier
        let mut new_hook = base_hook.clone();
        new_hook.set_identifier(1);

        // Make sure we can't process any messages
        assert_eq!(
            new_hook.process_against_average(100, MessageType::Broadcast),
            HookResult::SkipMessage
        );
    }

    #[test]
    fn test_sample() {
        // Create a new sample
        let mut sample = Sample::new();

        // Add 100 bytes
        sample.add(100.0);

        // Make sure we do not commit if we haven't waited long enough
        assert_eq!(sample.commit_if_necessary(Duration::from_millis(100)), None);

        // Wait 100ms
        sleep(Duration::from_millis(100));

        // Make sure we do commit
        let commit = sample
            .commit_if_necessary(Duration::from_millis(100))
            .expect("Failed to commit sample");

        // Make sure the bytes per period is approximately correct
        assert!((commit - 100.0).abs() < 5.0);

        // Make sure the state is reset
        assert!(sample.num_bytes == 0.0);
        assert!(sample.last_committed_time.elapsed() < Duration::from_millis(10));
    }

    #[test]
    fn test_average() {
        // Create a new average
        let average = Average::new();

        // Test that we don't commit with too few samples
        assert!(!average.commit_if_necessary(1));

        // Add 200 bytes/period to the average
        average.add(50.0);
        average.add(150.0);

        // Make sure we do commit
        assert!(average.commit_if_necessary(1));

        // Make sure the bytes per period is approximately correct
        assert_eq!(average.get_last_committed_average(), 100);

        // Make sure the internal state is reset
        assert!(*average.num_bytes.lock() == 0.0);
        assert_eq!(*average.num_samples.lock(), 0);

        // Do some more adding
        average.add(0.0);
        average.add(10.0);

        // Make sure we do commit again
        assert!(average.commit_if_necessary(2));

        // Make sure the bytes per period is approximately correct
        assert_eq!(average.get_last_committed_average(), 5);
    }

    #[test]
    #[ignore]
    fn test_deduplication_broadcast() {
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
    #[ignore]
    fn test_deduplication_direct() {
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
