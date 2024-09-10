#![allow(clippy::unnecessary_wraps)]
use std::hash::Hasher;
use std::marker::PhantomData;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use cdn_broker::reexports::def::hook::{HookResult, MessageHookDef};
use cdn_broker::reexports::message::{Broadcast, Direct, Message as PushCdnMessage};
use hotshot_types::traits::node_implementation::NodeType;
use lru::LruCache;
use parking_lot::Mutex;
use simple_moving_average::{SingleSumSMA, SMA as SmaTrait};
use twox_hash::xxh3::Hash64;

/// The minimum number of seconds before a sample is committed
const SAMPLE_COMMIT_INTERVAL_SECS: u64 = 100;

/// The number of messages received before checking against/updating the global average
const NUM_MESSAGES_BEFORE_CHECK: u64 = 5;

/// The multiple of the global average that the local average must be less than
const LOCAL_AVERAGE_MULTIPLE: u64 = 5;

/// A wrapper around an `SMA` type that allows for atomic
/// access of the previously calculated sum.
#[derive(Clone)]
struct SMA {
    /// The "inner" moving average object
    sma: Arc<Mutex<SingleSumSMA<u64, u64, 1000>>>,

    /// The previously calculated sum
    cached_sum: Arc<AtomicU64>,
}

impl SMA {
    /// Create a new `SMA`
    fn new() -> Self {
        Self {
            cached_sum: Arc::new(AtomicU64::new(0)),
            sma: Arc::new(Mutex::new(SingleSumSMA::new())),
        }
    }

    /// Commit a sample to the `SMA`. This will update the cached sum.
    fn commit_sample(&self, sample: &mut Sample) {
        // Calculate the sample's average bytes per second and reset the sample
        let bytes_per_second = sample.get();
        sample.reset();

        // Lock the `SMA`, add the sample, and get the new average
        let mut sma_guard = self.sma.lock();
        sma_guard.add_sample(bytes_per_second);
        let new_average = sma_guard.get_average();
        drop(sma_guard);

        // Store the new average in the cached sum
        self.cached_sum.store(new_average, Ordering::Relaxed);
    }

    /// Commit a sample if the
    fn commit_sample_if_elapsed(&self, sample: &mut Sample) {
        // Get the elapsed time since the last commit
        if sample.last_committed_time.elapsed().as_secs() >= SAMPLE_COMMIT_INTERVAL_SECS {
            self.commit_sample(sample);
        }
    }

    /// Get the cached (most currently updated) sum
    fn get(&self) -> u64 {
        self.sma.lock().get_average()
    }
}

/// A sample for the `SMA`. This is used to calculate the average bytes per second,
/// and is periodically committed and reset.
#[derive(Clone)]
struct Sample {
    /// The number of bytes sent since `last_committed_time`
    num_bytes_sent: u64,

    /// The number of messages sent since `last_committed_time`
    num_messages_sent: u64,

    /// The last time the sample was committed
    last_committed_time: Instant,
}

impl Sample {
    /// Create a new `Sample`
    fn new() -> Self {
        Self {
            num_bytes_sent: 0,
            num_messages_sent: 0,
            last_committed_time: Instant::now(),
        }
    }

    /// Add bytes to the sample and increment the number of messages sent
    fn add(&mut self, bytes: u64) {
        self.num_bytes_sent += bytes;
        self.num_messages_sent += 1;
    }

    /// Get the number of bytes per second of the current sample
    fn get(&self) -> u64 {
        self.num_bytes_sent
            .checked_div(self.last_committed_time.elapsed().as_secs())
            .unwrap_or(0)
    }

    /// Reset the sample. This is used when the sample is committed.
    fn reset(&mut self) {
        self.num_bytes_sent = 0;
        self.num_messages_sent = 0;
        self.last_committed_time = Instant::now();
    }
}

#[derive(Clone)]
/// The message hook for `HotShot` messages. Each user has a unique message hook.
pub struct HotShotMessageHook<T: NodeType> {
    /// The cache for message hashes. We use this to deduplicate a sliding window of
    /// 100 messages.
    message_hash_cache: LruCache<u64, ()>,

    /// The moving average for the global broadcast bytes per second
    global_broadcast_bps: SMA,

    /// The moving average for the global direct bytes per second
    global_direct_bps: SMA,

    /// The hook-local sample for the local broadcast bytes per second
    local_broadcast_bps: Sample,

    /// The hook-local sample for the local direct bytes per second
    local_direct_bps: Sample,

    /// The phantom data for the node type
    pd: PhantomData<T>,
}

impl<T: NodeType> Default for HotShotMessageHook<T> {
    /// # Panics
    /// If 100 < 0
    fn default() -> Self {
        Self {
            global_broadcast_bps: SMA::new(),
            global_direct_bps: SMA::new(),
            local_broadcast_bps: Sample::new(),
            local_direct_bps: Sample::new(),
            message_hash_cache: LruCache::new(NonZeroUsize::new(100).unwrap()),
            pd: PhantomData,
        }
    }
}

impl<T: NodeType> HotShotMessageHook<T> {
    /// Create a new `HotShotMessageHook`
    ///
    /// # Panics
    /// If 100 < 0
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Process incoming broadcast messages from the user
    fn process_broadcast_message(&mut self, broadcast: &mut Broadcast) -> Result<HookResult> {
        // Add the number of bytes in the message to the sample average
        self.local_broadcast_bps.add(broadcast.message.len() as u64);

        // For every 5 messages, check against the global average and potentially commit the sample
        if self.local_broadcast_bps.num_messages_sent % NUM_MESSAGES_BEFORE_CHECK == 0
            && self.local_broadcast_bps.num_messages_sent != 0
        {
            // Commit the sample if 100 seconds have passed
            self.global_broadcast_bps
                .commit_sample_if_elapsed(&mut self.local_broadcast_bps);

            // Make sure our local average is not too high
            let local_bps = self.local_broadcast_bps.get();
            let global_bps = self.global_broadcast_bps.get();
            if global_bps != 0 && local_bps > global_bps * LOCAL_AVERAGE_MULTIPLE {
                return Ok(HookResult::SkipMessage);
            }
        }

        // Calculate the hash of the message
        let mut hasher = Hash64::default();
        hasher.write(&broadcast.message);
        hasher.write(&broadcast.topics);

        // Make sure we have not already seen it
        if self.message_hash_cache.put(hasher.finish(), ()).is_some() {
            return Ok(HookResult::SkipMessage);
        }

        // Make sure it is deserializable
        // let (_, _) = Self::deserialize_message(&broadcast.message)?;

        Ok(HookResult::ProcessMessage)
    }

    /// Process incoming direct messages from the user
    fn process_direct_message(&mut self, direct: &mut Direct) -> Result<HookResult> {
        // Add the number of bytes in the message to the local average
        self.local_direct_bps.add(direct.message.len() as u64);

        // For every 5 messages, check against the global average and potentially commit the sample
        if self.local_direct_bps.num_messages_sent % NUM_MESSAGES_BEFORE_CHECK == 0
            && self.local_direct_bps.num_messages_sent != 0
        {
            // Commit the sample if 100 seconds have passed
            self.global_direct_bps
                .commit_sample_if_elapsed(&mut self.local_direct_bps);

            // Make sure our local average is not too high
            let local_bps = self.local_direct_bps.get();
            let global_bps = self.global_direct_bps.get();
            if global_bps != 0 && local_bps > global_bps * LOCAL_AVERAGE_MULTIPLE {
                return Ok(HookResult::SkipMessage);
            }
        }

        // Calculate the hash of the message
        let mut hasher = Hash64::default();
        hasher.write(&direct.message);
        hasher.write(&direct.recipient);

        // Make sure we have not already seen it
        if self.message_hash_cache.put(hasher.finish(), ()).is_some() {
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
