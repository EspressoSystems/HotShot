// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Vote tracking implementation for preventing double voting

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::traits::node_implementation::NodeType;
use crate::traits::signature_key::SignatureKey;

/// Tracks votes to prevent double voting in HotShot consensus
#[derive(Debug, Default)]
pub struct VoteTracker<TYPES: NodeType> {
    /// Maps view numbers to sets of voter public keys
    view_votes: HashMap<TYPES::View, HashSet<Arc<TYPES::SignatureKey>>>,
}

impl<TYPES: NodeType> VoteTracker<TYPES> {
    /// Creates a new VoteTracker instance
    pub fn new() -> Self {
        Self {
            view_votes: HashMap::new(),
        }
    }

    /// Records a vote for a specific view and voter
    /// Returns true if the vote was successfully recorded (no double voting)
    /// Returns false if this would be a double vote
    pub fn record_vote(
        &mut self,
        view: TYPES::View,
        voter_key: Arc<TYPES::SignatureKey>,
    ) -> bool {
        // Get or create the vote set for this view
        let votes = self.view_votes.entry(view).or_default();
        
        // Check if this voter has already voted
        if votes.contains(&voter_key) {
            return false;
        }

        // Record the vote
        votes.insert(voter_key);
        true
    }

    /// Cleans up old vote records for views that are no longer needed
    /// This should be called periodically to prevent memory growth
    pub fn cleanup_old_views(&mut self, current_view: TYPES::View) {
        self.view_votes.retain(|view, _| *view >= current_view);
    }

    /// Returns true if the given voter has already voted in the specified view
    pub fn has_voted(
        &self,
        view: &TYPES::View,
        voter_key: &Arc<TYPES::SignatureKey>,
    ) -> bool {
        self.view_votes
            .get(view)
            .map_or(false, |votes| votes.contains(voter_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simple_vote::SimpleVote;
    use std::sync::Arc;

    #[test]
    fn test_vote_tracking() {
        let mut tracker = VoteTracker::<SimpleVote>::new();
        let view = SimpleVote::View::default();
        let key1 = Arc::new(SimpleVote::SignatureKey::default());
        let key2 = Arc::new(SimpleVote::SignatureKey::default());

        // First vote should succeed
        assert!(tracker.record_vote(view, key1.clone()));
        
        // Double vote should fail
        assert!(!tracker.record_vote(view, key1.clone()));
        
        // Different voter should succeed
        assert!(tracker.record_vote(view, key2.clone()));
    }

    #[test]
    fn test_cleanup() {
        let mut tracker = VoteTracker::<SimpleVote>::new();
        let view1 = SimpleVote::View::default();
        let view2 = SimpleVote::View::default() + SimpleVote::View::from(1);
        let key = Arc::new(SimpleVote::SignatureKey::default());

        tracker.record_vote(view1, key.clone());
        tracker.record_vote(view2, key.clone());

        // Cleanup views before view2
        tracker.cleanup_old_views(view2);

        // Old vote should be removed
        assert!(!tracker.has_voted(&view1, &key));
        // Recent vote should remain
        assert!(tracker.has_voted(&view2, &key));
    }
}
