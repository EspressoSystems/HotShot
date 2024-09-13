#![allow(clippy::unnecessary_wraps)]
use std::hash::Hasher;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use cdn_broker::reexports::def::hook::{HookResult, MessageHookDef};
use cdn_broker::reexports::message::{Broadcast, Direct, Message as PushCdnMessage};
use lru::LruCache;
use parking_lot::Mutex;
use simple_moving_average::{SingleSumSMA, SMA as SmaTrait};
use std::time::Duration;
use tracing::warn;
use twox_hash::xxh3::Hash64;

/// A wrapper around an `SMA` type that allows for atomic
/// access of the previously calculated sum.
#[derive(Clone)]
struct Sma {
    /// The "inner" moving average object
    sma: Arc<Mutex<SingleSumSMA<u64, u64, 1000>>>,

    /// The previously calculated sum
    cached_sum: Arc<AtomicU64>,
}

/// The type of message being processed. Is used downstream to determine
/// which sample and average to use when processing a message.
#[derive(Eq, PartialEq, Clone, Copy)]
enum MessageType {
    /// A broadcast message
    Broadcast,
    /// A direct message
    Direct,
}

impl Sma {
    /// Create a new `SMA`
    fn new() -> Self {
        Self {
            cached_sum: Arc::new(AtomicU64::new(0)),
            sma: Arc::new(Mutex::new(SingleSumSMA::new())),
        }
    }

    /// Commit a sample to the `SMA`. This will update the cached sum.
    fn commit_sample(&mut self, sample: &mut Sample) {
        // Calculate the sample's average bytes per second and reset the sample
        let bytes_per_second = sample.get();
        sample.reset();

        // Lock the `SMA`, add the sample, and get the new average
        let mut sma_guard = self.sma.lock();
        sma_guard.add_sample(bytes_per_second);
        let new_average = sma_guard.get_average();

        // Store the new average in the cached sum
        self.cached_sum.store(new_average, Ordering::Relaxed);
        drop(sma_guard);
    }

    /// Get the cached (most currently updated) sum
    fn get(&self) -> u64 {
        self.cached_sum.load(Ordering::Relaxed)
    }
}

/// A sample for the `SMA`. This is used to calculate the average bytes per second,
/// and is periodically committed and reset.
#[derive(Clone)]
struct Sample {
    /// The number of bytes sent since `last_committed_time`
    num_bytes_sent: u64,

    /// When we should start processing messages again
    cooldown_until: Instant,

    /// The last time the sample was checked
    last_checked_time: Instant,

    /// The last time the sample was committed
    last_committed_time: Instant,
}

impl Sample {
    /// Create a new `Sample`
    fn new() -> Self {
        Self {
            num_bytes_sent: 0,
            cooldown_until: Instant::now(),
            last_checked_time: Instant::now(),
            last_committed_time: Instant::now(),
        }
    }

    /// Add bytes to the sample and increment the number of messages sent
    fn add(&mut self, bytes: u64) {
        self.num_bytes_sent += bytes;
    }

    /// Get the number of bytes per second of the current sample
    fn get(&self) -> u64 {
        self.num_bytes_sent / self.last_committed_time.elapsed().as_secs().max(1)
    }

    /// Reset the sample. This is used when the sample is committed.
    fn reset(&mut self) {
        self.num_bytes_sent = 0;
        self.last_checked_time = Instant::now();
        self.last_committed_time = Instant::now();
    }
}

#[derive(Clone)]
/// The message hook for `HotShot` messages. Each user has a unique message hook.
pub struct HotShotMessageHook {
    /// The cache for message hashes. We use this to deduplicate a sliding window of
    /// 100 messages.
    message_hash_cache: LruCache<u64, ()>,

    /// The sample check interval
    sample_check_interval: Duration,

    /// The sample commit interval
    sample_commit_interval: Duration,

    /// The multiple of our average that the local average is allowed to be
    allowed_multiple: u64,

    /// The global moving average for the number of broadcast bytes per second
    global_broadcast_bps: Sma,

    /// The local average for the number of broadcast bytes per second
    local_broadcast_bps: Sample,

    /// The global moving average for the number of direct bytes per second
    global_direct_bps: Sma,

    /// The local average for the number of direct bytes per second
    local_direct_bps: Sample,
}

impl Default for HotShotMessageHook {
    /// # Panics
    /// If 100 < 0
    fn default() -> Self {
        Self {
            sample_check_interval: Duration::from_secs(5),
            sample_commit_interval: Duration::from_secs(120),
            allowed_multiple: 4,

            global_broadcast_bps: Sma::new(),
            global_direct_bps: Sma::new(),
            local_broadcast_bps: Sample::new(),
            local_direct_bps: Sample::new(),
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
    pub fn new(
        sample_check_interval: Duration,
        sample_commit_interval: Duration,
        allowed_multiple: u64,
    ) -> Self {
        Self {
            sample_check_interval,
            sample_commit_interval,
            allowed_multiple,

            global_broadcast_bps: Sma::new(),
            global_direct_bps: Sma::new(),
            local_broadcast_bps: Sample::new(),
            local_direct_bps: Sample::new(),

            message_hash_cache: LruCache::new(NonZeroUsize::new(100).unwrap()),
        }
    }

    /// Process a message against the moving average
    /// Returns whether or not the message should be skipped.
    fn process_against_sma(&mut self, message_len: usize, message_type: MessageType) -> HookResult {
        // Match the sample and `SMA` based on the message type
        let (sample, sma) = match message_type {
            MessageType::Broadcast => (
                &mut self.local_broadcast_bps,
                &mut self.global_broadcast_bps,
            ),
            MessageType::Direct => (&mut self.local_direct_bps, &mut self.global_direct_bps),
        };

        // Get the current time
        let now = Instant::now();

        // Commit the sample if that interval has elapsed
        if now.duration_since(sample.last_committed_time) >= self.sample_commit_interval {
            sma.commit_sample(sample);
        }

        // Skip the message if we need to cool down
        if sample.cooldown_until > now {
            return HookResult::SkipMessage;
        }

        // Add the length to the local sample
        sample.add(message_len as u64);

        // If we have surpassed the check interval, check the sample to make sure it does
        // not exceed the `global average * allowed_multiple`
        if now.duration_since(sample.last_checked_time) >= self.sample_check_interval {
            // Get our local and global bps
            let local_bps = sample.get();
            let mut global_bps = sma.get();

            // Clamp the global bps to a minimum if it's not zero (meaning uninitialized)
            if global_bps > 0 {
                global_bps = std::cmp::max(global_bps, 5000);
            }

            // Calculate the maximum allowed bps
            let max_allowed_bps = global_bps * self.allowed_multiple;

            // If the local bps is greater than the allowed bps, calculate the cooldown and skip
            // the message
            if global_bps != 0 && local_bps > max_allowed_bps {
                // Set the cooldown to the time it would take to get the local bps to the max allowed
                sample.cooldown_until =
                    Instant::now() + Duration::from_secs(local_bps / max_allowed_bps);

                // Skip the message
                return HookResult::SkipMessage;
            }

            // Reset the check time
            sample.last_checked_time = Instant::now();
        }

        // Commit the sample if that interval has elapsed
        if now.duration_since(sample.last_committed_time) >= self.sample_commit_interval {
            sma.commit_sample(sample);
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
        // Process through the `SMA`. Skip if it's over the threshold
        let HookResult::ProcessMessage =
            self.process_against_sma(broadcast.message.len(), MessageType::Broadcast)
        else {
            warn!("Broadcast message not processed due to high message rate");
            return Ok(HookResult::SkipMessage);
        };

        // Skip the message if we've already seen it
        if self.message_already_seen(&broadcast.message, &broadcast.topics) {
            return Ok(HookResult::SkipMessage);
        }

        // Make sure it is deserializable
        // let (_, _) = Self::deserialize_message(&broadcast.message)?;

        Ok(HookResult::ProcessMessage)
    }

    /// Process incoming direct messages from the user
    fn process_direct_message(&mut self, direct: &mut Direct) -> Result<HookResult> {
        // Process through the `SMA`. Skip if it's over the threshold
        let HookResult::ProcessMessage =
            self.process_against_sma(direct.message.len(), MessageType::Direct)
        else {
            warn!("Direct message not processed due to high message rate");
            return Ok(HookResult::SkipMessage);
        };

        // Skip the message if we've already seen it
        if self.message_already_seen(&direct.message, &direct.recipient) {
            return Ok(HookResult::SkipMessage);
        }

        // Make sure it is deserializable
        // let (_, _) = Self::deserialize_message(&direct.message)?;

        Ok(HookResult::ProcessMessage)
    }

    // fn deserialize_message(message: &[u8]) -> Result<(Message<T>, Version)> {
    //     // Hack off the version
    //     let (version, message) =
    //         Version::deserialize(&message).with_context(|| "failed to deserialize message")?;

    //     // Deserialize the message
    //     let message = Serializer::<StaticVersion<0, 1>>::deserialize_no_version(&message)
    //         .with_context(|| "failed to deserialize message")?;

    //     // Return the version and message
    //     Ok((message, version))
    // }
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

    #[test]
    fn in_range() {
        // Create a new message hook
        let mut hook = HotShotMessageHook {
            sample_check_interval: Duration::from_secs(1),
            allowed_multiple: 1,
            global_broadcast_bps: Sma {
                // Pretend we've seen an average of 5000 bytes per second
                cached_sum: Arc::new(AtomicU64::new(5000)),
                sma: Arc::new(Mutex::new(SingleSumSMA::new())),
            },
            local_broadcast_bps: Sample {
                num_bytes_sent: 0,
                cooldown_until: Instant::now(),
                // Pretend we need to check the sample
                last_checked_time: Instant::now().checked_sub(Duration::from_secs(1)).unwrap(),
                last_committed_time: Instant::now(),
            },
            ..Default::default()
        };

        // Create a message just within the range
        let message = vec![0; 4800];
        let mut broadcast = Broadcast {
            message,
            topics: vec![],
        };

        // Process the message, make sure it would've been sent
        let result = hook.process_broadcast_message(&mut broadcast);
        assert!(
            result.is_ok(),
            "Message should have been processed but was not",
        );
    }

    #[test]
    fn exceeding_range() {
        // Create a new message hook
        let mut hook = HotShotMessageHook {
            sample_check_interval: Duration::from_secs(1),
            allowed_multiple: 1,
            global_broadcast_bps: Sma {
                // Pretend we've seen an average of 5000 bytes per second
                cached_sum: Arc::new(AtomicU64::new(5000)),
                sma: Arc::new(Mutex::new(SingleSumSMA::new())),
            },
            local_broadcast_bps: Sample {
                num_bytes_sent: 0,
                cooldown_until: Instant::now(),
                // Pretend we need to check the sample
                last_checked_time: Instant::now().checked_sub(Duration::from_secs(1)).unwrap(),
                last_committed_time: Instant::now(),
            },
            ..Default::default()
        };

        // Create a message exceeding the range
        let message = vec![0; 10000];
        let mut broadcast = Broadcast {
            message,
            topics: vec![],
        };

        // Process the message, make sure it would've been sent
        let result = hook.process_broadcast_message(&mut broadcast);
        assert!(
            result.unwrap() == HookResult::SkipMessage,
            "Message should have been skipped but was not",
        );

        // Wait one second, make sure it's still skipped
        let message = vec![1; 10000];
        let mut broadcast = Broadcast {
            message,
            topics: vec![],
        };
        std::thread::sleep(Duration::from_millis(1000));
        let result = hook.process_broadcast_message(&mut broadcast);
        assert!(
            result.unwrap() == HookResult::SkipMessage,
            "Message should have been skipped but was not",
        );

        // Wait another second and some change, make sure it's processed
        let message = vec![2; 1];
        let mut broadcast = Broadcast {
            message,
            topics: vec![],
        };
        std::thread::sleep(Duration::from_millis(1500));
        let result = hook.process_broadcast_message(&mut broadcast);
        assert!(
            result.unwrap() == HookResult::ProcessMessage,
            "Message should have been processed but was not",
        );
    }
}
