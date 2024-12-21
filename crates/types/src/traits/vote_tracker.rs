// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Vote tracking implementation for preventing double voting

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::num::NonZeroU64;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::traits::node_implementation::NodeType;
use crate::traits::signature_key::SignatureKey;
use thiserror::Error;

/// Tracks votes to prevent double voting in HotShot consensus
#[derive(Debug)]
pub struct VoteTracker<TYPES: NodeType> {
    /// Maps view numbers to sets of voter public keys
    view_votes: HashMap<TYPES::View, HashSet<Arc<TYPES::SignatureKey>>>,
    /// Maps voter public keys to their last voted view
    last_vote_views: HashMap<Arc<TYPES::SignatureKey>, TYPES::View>,
    /// Track view transitions for each voter
    view_transitions: HashMap<Arc<TYPES::SignatureKey>, VecDeque<TYPES::View>>,
    /// Track view sequence and pending votes
    view_sequence: ViewSequence<TYPES>,
    /// Minimum valid view number
    min_valid_view: TYPES::View,
    /// Window size for vote tracking (number of views to keep)
    window_size: NonZeroU64,
    /// Maximum allowed view jump to prevent large view number attacks
    max_view_jump: NonZeroU64,
    /// Maximum number of pending transitions
    max_pending_transitions: usize,
    /// Metrics for monitoring vote operations
    metrics: VoteMetrics,
}

/// Tracks view sequence and manages pending votes
#[derive(Debug, Default)]
struct ViewSequence<TYPES: NodeType> {
    /// Track all views that have been voted on (ordered)
    voted_views: BTreeSet<TYPES::View>,
    /// Track pending votes that are waiting for previous views
    pending_votes: BTreeMap<TYPES::View, HashSet<Arc<TYPES::SignatureKey>>>,
    /// Track the minimum required view for each voter
    min_required_views: HashMap<Arc<TYPES::SignatureKey>, TYPES::View>,
    /// Track complete view sequences for each voter
    voter_sequences: HashMap<Arc<TYPES::SignatureKey>, BTreeSet<TYPES::View>>,
}

/// Metrics for monitoring vote operations
#[derive(Debug, Default)]
struct VoteMetrics {
    /// Total number of votes processed
    total_votes: u64,
    /// Number of rejected votes
    rejected_votes: u64,
    /// Last cleanup timestamp
    last_cleanup: SystemTime,
    /// Number of out-of-order votes detected
    out_of_order_votes: u64,
    /// Number of pending votes
    pending_votes: u64,
}

#[derive(Debug, Error)]
pub enum VoteError {
    #[error("View number is too old")]
    ViewTooOld,
    #[error("Already voted in this or later view")]
    AlreadyVoted,
    #[error("Invalid view jump detected")]
    ViewJumpTooLarge,
    #[error("Invalid epoch")]
    InvalidEpoch,
    #[error("Invalid cleanup operation")]
    InvalidCleanup,
    #[error("View number overflow detected")]
    ViewNumberOverflow,
    #[error("Missing vote in previous view {0}")]
    MissingPreviousVote(u64),
    #[error("Non-sequential vote detected")]
    NonSequentialVote,
    #[error("View transition violation")]
    InvalidViewTransition,
    #[error("View sequence violation")]
    InvalidViewSequence,
}

impl<TYPES: NodeType> ViewSequence<TYPES> {
    /// Creates a new ViewSequence
    fn new() -> Self {
        Self::default()
    }

    /// Checks if a vote follows the sequential order
    fn is_sequential(&self, view: TYPES::View, voter_key: &Arc<TYPES::SignatureKey>) -> Result<bool, VoteError> {
        let voter_seq = self.voter_sequences.get(voter_key).unwrap_or(&BTreeSet::new());
        
        // If this is the first vote, it's sequential
        if voter_seq.is_empty() {
            return Ok(true);
        }

        // Get the last voted view for this voter
        if let Some(&last_view) = voter_seq.iter().last() {
            // Check if there are any gaps
            let expected_view = last_view + TYPES::View::from(1);
            if view != expected_view {
                tracing::debug!(
                    "Non-sequential vote detected: expected view {}, got {}",
                    expected_view.as_u64(),
                    view.as_u64()
                );
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Queues a vote for later processing
    fn queue_vote(&mut self, view: TYPES::View, voter_key: Arc<TYPES::SignatureKey>) -> Result<(), VoteError> {
        let pending = self.pending_votes.entry(view).or_default();
        pending.insert(voter_key);
        Ok(())
    }

    /// Records a successful vote
    fn record_successful_vote(&mut self, view: TYPES::View, voter_key: &Arc<TYPES::SignatureKey>) -> Result<(), VoteError> {
        // Update voted views
        self.voted_views.insert(view);
        
        // Update voter sequence
        let voter_seq = self.voter_sequences.entry(voter_key.clone()).or_default();
        voter_seq.insert(view);
        
        // Update minimum required view
        self.min_required_views.insert(voter_key.clone(), view + TYPES::View::from(1));
        
        Ok(())
    }

    /// Processes any pending votes that can now be applied
    fn process_pending_votes(&mut self, voter_key: &Arc<TYPES::SignatureKey>) -> Result<Vec<TYPES::View>, VoteError> {
        let mut processed_views = Vec::new();
        let mut views_to_process: Vec<_> = self.pending_votes.keys().cloned().collect();
        views_to_process.sort();

        for view in views_to_process {
            if let Some(voters) = self.pending_votes.get_mut(&view) {
                if voters.contains(voter_key) && self.is_sequential(view, voter_key)? {
                    voters.remove(voter_key);
                    self.record_successful_vote(view, voter_key)?;
                    processed_views.push(view);
                    
                    if voters.is_empty() {
                        self.pending_votes.remove(&view);
                    }
                }
            }
        }

        Ok(processed_views)
    }

    /// Gets the minimum required view for a voter
    fn get_min_required_view(&self, voter_key: &Arc<TYPES::SignatureKey>) -> TYPES::View {
        self.min_required_views
            .get(voter_key)
            .cloned()
            .unwrap_or_else(|| TYPES::View::genesis())
    }

    /// Cleans up old view data
    fn cleanup(&mut self, min_valid_view: TYPES::View) {
        // Clean up voted views
        self.voted_views.retain(|&view| view >= min_valid_view);
        
        // Clean up pending votes
        self.pending_votes.retain(|&view, _| view >= min_valid_view);
        
        // Clean up voter sequences
        for seq in self.voter_sequences.values_mut() {
            seq.retain(|&view| view >= min_valid_view);
        }
        
        // Update minimum required views
        for min_view in self.min_required_views.values_mut() {
            if *min_view < min_valid_view {
                *min_view = min_valid_view;
            }
        }
    }
}

impl<TYPES: NodeType> VoteTracker<TYPES> {
    /// Creates a new VoteTracker instance
    pub fn new(window_size: NonZeroU64, max_view_jump: NonZeroU64) -> Self {
        Self {
            view_votes: HashMap::new(),
            last_vote_views: HashMap::new(),
            view_transitions: HashMap::new(),
            view_sequence: ViewSequence::new(),
            min_valid_view: TYPES::View::genesis(),
            window_size,
            max_view_jump,
            max_pending_transitions: 10,
            metrics: VoteMetrics::default(),
        }
    }

    /// Records a vote for a specific view and voter with strict ordering checks
    pub fn record_vote(
        &mut self,
        view: TYPES::View,
        voter_key: Arc<TYPES::SignatureKey>,
        current_epoch: TYPES::Epoch,
    ) -> Result<bool, VoteError> {
        // Check for integer overflow
        if view.as_u64() == u64::MAX {
            self.metrics.rejected_votes += 1;
            return Err(VoteError::ViewNumberOverflow);
        }

        // Validate view number against minimum valid view
        if view < self.min_valid_view {
            self.metrics.rejected_votes += 1;
            return Err(VoteError::ViewTooOld);
        }

        // Check for previous votes and validate view sequence
        if let Some(last_view) = self.last_vote_views.get(&voter_key) {
            if view <= *last_view {
                self.metrics.rejected_votes += 1;
                return Err(VoteError::AlreadyVoted);
            }

            // Validate view transition
            self.validate_view_transition(view, &voter_key)?;
        }

        // Check for sequential voting
        if !self.view_sequence.is_sequential(view, &voter_key)? {
            self.metrics.out_of_order_votes += 1;
            
            // Queue the vote for later processing
            self.view_sequence.queue_vote(view, voter_key.clone())?;
            self.metrics.pending_votes += 1;
            
            return Err(VoteError::NonSequentialVote);
        }

        // Record the vote in the view sequence
        self.view_sequence.record_successful_vote(view, &voter_key)?;

        // Process any pending votes that can now be applied
        let processed_views = self.view_sequence.process_pending_votes(&voter_key)?;
        self.metrics.pending_votes = self.metrics.pending_votes.saturating_sub(processed_views.len() as u64);

        // Update vote tracking state
        self.last_vote_views.insert(voter_key.clone(), view);
        let votes = self.view_votes.entry(view).or_default();
        votes.insert(voter_key.clone());

        // Update view transition history
        let transitions = self.view_transitions.entry(voter_key).or_default();
        transitions.push_back(view);
        while transitions.len() > self.max_pending_transitions {
            transitions.pop_front();
        }

        self.metrics.total_votes += 1;
        Ok(true)
    }

    /// Validates the transition between views for a voter
    fn validate_view_transition(
        &mut self,
        view: TYPES::View,
        voter_key: &Arc<TYPES::SignatureKey>,
    ) -> Result<(), VoteError> {
        let transitions = self.view_transitions.entry(voter_key.clone()).or_default();
        
        if let Some(&last_view) = transitions.back() {
            // Check for sequential voting
            if view <= last_view {
                return Err(VoteError::NonSequentialVote);
            }

            // Check for too large jumps
            let view_diff = view.as_u64().saturating_sub(last_view.as_u64());
            if view_diff > self.max_view_jump.get() {
                return Err(VoteError::ViewJumpTooLarge);
            }
        }

        Ok(())
    }

    /// Checks if a voter has voted in a specific view
    fn has_voted_in_view(&self, voter_key: &Arc<TYPES::SignatureKey>, view: &TYPES::View) -> bool {
        self.view_votes
            .get(view)
            .map_or(false, |votes| votes.contains(voter_key))
    }

    /// Cleans up old vote records with safety checks
    pub fn cleanup_old_views(&mut self, current_view: TYPES::View) -> Result<(), VoteError> {
        let new_min_view = current_view.saturating_sub(TYPES::View::from(self.window_size.get()));
        if new_min_view < self.min_valid_view {
            return Err(VoteError::InvalidCleanup);
        }

        self.min_valid_view = new_min_view;

        // Clean up old records while maintaining vote history integrity
        self.view_votes.retain(|view, _| *view >= self.min_valid_view);
        self.last_vote_views.retain(|_, last_view| *last_view >= self.min_valid_view);
        
        // Clean up view transitions
        for transitions in self.view_transitions.values_mut() {
            while let Some(front) = transitions.front() {
                if *front < new_min_view {
                    transitions.pop_front();
                } else {
                    break;
                }
            }
        }

        // Clean up view sequence data
        self.view_sequence.cleanup(new_min_view);

        self.metrics.last_cleanup = SystemTime::now();
        Ok(())
    }

    /// Get current metrics for monitoring
    pub fn get_metrics(&self) -> &VoteMetrics {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simple_vote::SimpleVote;

    #[test]
    fn test_sequential_voting() {
        let mut tracker = VoteTracker::<SimpleVote>::new(
            NonZeroU64::new(10).unwrap(),
            NonZeroU64::new(5).unwrap()
        );
        let key = Arc::new(SimpleVote::SignatureKey::default());

        // First vote should succeed
        assert!(tracker.record_vote(
            SimpleVote::View::from(1),
            key.clone(),
            SimpleVote::Epoch::default()
        ).is_ok());

        // Non-sequential vote should fail
        assert!(tracker.record_vote(
            SimpleVote::View::from(3),
            key.clone(),
            SimpleVote::Epoch::default()
        ).is_err());

        // Sequential vote should succeed
        assert!(tracker.record_vote(
            SimpleVote::View::from(2),
            key.clone(),
            SimpleVote::Epoch::default()
        ).is_ok());
    }

    #[test]
    fn test_pending_votes() {
        let mut tracker = VoteTracker::<SimpleVote>::new(
            NonZeroU64::new(10).unwrap(),
            NonZeroU64::new(5).unwrap()
        );
        let key = Arc::new(SimpleVote::SignatureKey::default());

        // Vote for view 2 should be queued
        let result = tracker.record_vote(
            SimpleVote::View::from(2),
            key.clone(),
            SimpleVote::Epoch::default()
        );
        assert!(result.is_err());
        assert_eq!(tracker.metrics.pending_votes, 1);

        // Vote for view 1 should succeed and process pending vote
        assert!(tracker.record_vote(
            SimpleVote::View::from(1),
            key.clone(),
            SimpleVote::Epoch::default()
        ).is_ok());
        
        // Verify metrics
        assert_eq!(tracker.metrics.pending_votes, 0);
        assert_eq!(tracker.metrics.total_votes, 2);
    }

    #[test]
    fn test_view_transition_limits() {
        let mut tracker = VoteTracker::<SimpleVote>::new(
            NonZeroU64::new(10).unwrap(),
            NonZeroU64::new(5).unwrap()
        );
        let key = Arc::new(SimpleVote::SignatureKey::default());

        // Sequential votes within limit should succeed
        for i in 1..=5 {
            assert!(tracker.record_vote(
                SimpleVote::View::from(i),
                key.clone(),
                SimpleVote::Epoch::default()
            ).is_ok());
        }

        // Vote with too large jump should fail
        assert!(tracker.record_vote(
            SimpleVote::View::from(11),
            key.clone(),
            SimpleVote::Epoch::default()
        ).is_err());
    }

    #[test]
    fn test_cross_epoch_voting() {
        let mut tracker = VoteTracker::<SimpleVote>::new(
            NonZeroU64::new(10).unwrap(),
            NonZeroU64::new(5).unwrap()
        );
        let key = Arc::new(SimpleVote::SignatureKey::default());
        
        // Vote in first epoch
        assert!(tracker.record_vote(
            SimpleVote::View::from(1),
            key.clone(),
            SimpleVote::Epoch::from(1)
        ).is_ok());
        
        // Vote in next epoch should reset view sequence
        assert!(tracker.record_vote(
            SimpleVote::View::from(1),
            key.clone(),
            SimpleVote::Epoch::from(2)
        ).is_ok());
        
        // Continue voting in new epoch
        assert!(tracker.record_vote(
            SimpleVote::View::from(2),
            key.clone(),
            SimpleVote::Epoch::from(2)
        ).is_ok());
    }

    #[test]
    fn test_recovery_from_nonsequential() {
        let mut tracker = VoteTracker::<SimpleVote>::new(
            NonZeroU64::new(10).unwrap(),
            NonZeroU64::new(5).unwrap()
        );
        let key = Arc::new(SimpleVote::SignatureKey::default());
        
        // Vote 1 succeeds
        assert!(tracker.record_vote(
            SimpleVote::View::from(1),
            key.clone(),
            SimpleVote::Epoch::default()
        ).is_ok());
        
        // Vote 3 is queued
        let result = tracker.record_vote(
            SimpleVote::View::from(3),
            key.clone(),
            SimpleVote::Epoch::default()
        );
        assert!(result.is_err());
        assert_eq!(tracker.metrics.pending_votes, 1);
        
        // Vote 2 arrives and triggers processing of vote 3
        assert!(tracker.record_vote(
            SimpleVote::View::from(2),
            key.clone(),
            SimpleVote::Epoch::default()
        ).is_ok());
        
        assert_eq!(tracker.metrics.pending_votes, 0);
        assert_eq!(tracker.metrics.total_votes, 3);
    }
}
