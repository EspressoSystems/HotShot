// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::collections::BTreeSet;

use rand::{rngs::StdRng, Rng, SeedableRng};

/// Helper which allows producing random numbers within a range and preventing duplicates
/// If consumed as a regular iterator, will return a randomly ordered permutation of all
/// values from 0..max
struct NonRepeatValueIterator {
    /// Random number generator to use
    rng: StdRng,

    /// Values which have already been emitted, to avoid duplicates
    values: BTreeSet<u64>,

    /// Maximum value, open-ended. Numbers returned will be 0..max
    max: u64,
}

impl NonRepeatValueIterator {
    /// Create a new NonRepeatValueIterator
    pub fn new(rng: StdRng, max: u64) -> Self {
        Self {
            rng,
            values: BTreeSet::new(),
            max,
        }
    }
}

impl Iterator for NonRepeatValueIterator {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.values.len() as u64 >= self.max {
            return None;
        }

        loop {
            let v = self.rng.gen_range(0..self.max);
            if !self.values.contains(&v) {
                self.values.insert(v);
                return Some(v);
            }
        }
    }
}

/// Create a single u64 seed by merging two u64s. Done this way to allow easy seeding of the number generator
/// from both a stable SOUND as well as a moving value ROUND (typically, epoch).
fn make_seed(seed: u64, round: u64) -> u64 {
    seed.wrapping_add(round.wrapping_shl(8))
}

/// Create a pair of PRNGs for the given SEED and ROUND. Prev_rng is the PRNG for the previous ROUND, used to
/// deterministically replay random numbers generated for the previous ROUND.
fn make_rngs(seed: u64, round: u64) -> (StdRng, StdRng) {
    let prev_rng = SeedableRng::seed_from_u64(make_seed(seed, round.wrapping_sub(1)));
    let this_rng = SeedableRng::seed_from_u64(make_seed(seed, round));

    (prev_rng, this_rng)
}

/// Iterator which returns odd/even values for a given COUNT of nodes. For OVERLAP=0, this will return
/// [0, 2, 4, 6, ...] for an even round, and [1, 3, 5, 7, ...] for an odd round. Setting OVERLAP>0 will
/// randomly introduce OVERLAP elements from the previous round, so an even round with OVERLAP=2 will contain
/// something like [1, 7, 2, 4, 0, ...]. Note that the total number of nodes will always be COUNT/2, so
/// for OVERLAP>0 a random number of nodes which would have been in the round for OVERLAP=0 will be dropped.
/// Ordering of nodes is random. Outputs is deterministic when prev_rng and this_rng are provided by make_rngs
/// using the same values for SEED and ROUND.
pub struct StableQuorumIterator {
    /// PRNG from the previous round
    prev_rng: NonRepeatValueIterator,

    /// PRNG for the current round
    this_rng: NonRepeatValueIterator,

    /// Current ROUND
    round: u64,

    /// Count of nodes in the source quorum being filtered against
    count: u64,

    /// OVERLAP of nodes to be carried over from the previous round
    overlap: u64,

    /// The next call to next() will emit the value with this index. Starts at 0 and is incremented for each
    /// call to next()
    index: u64,
}

/// Determines how many possible values can be made for the given odd/even
/// E.g. if count is 5, then possible values would be [0, 1, 2, 3, 4]
/// if odd = true, slots = 2 (1 or 3), else slots = 3 (0, 2, 4)
fn calc_num_slots(count: u64, odd: bool) -> u64 {
    (count / 2) + if odd { count % 2 } else { 0 }
}

impl StableQuorumIterator {
    /// Create a new StableQuorumIterator
    pub fn new(seed: u64, round: u64, count: u64, overlap: u64) -> Self {
        assert!(
            count / 2 > overlap,
            "Overlap cannot be greater than the entire set size"
        );

        let (prev_rng, this_rng) = make_rngs(seed, round);

        Self {
            prev_rng: NonRepeatValueIterator::new(prev_rng, calc_num_slots(count, round % 2 == 0)),
            this_rng: NonRepeatValueIterator::new(this_rng, calc_num_slots(count, round % 2 == 1)),
            round,
            count,
            overlap,
            index: 0,
        }
    }
}

impl Iterator for StableQuorumIterator {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= (self.count / 2) {
            None
        } else if self.index < self.overlap {
            // Generate enough values for the previous round
            let v = self.prev_rng.next().unwrap();
            self.index += 1;
            Some(v * 2 + self.round % 2)
        } else {
            // Generate new values
            let v = self.this_rng.next().unwrap();
            self.index += 1;
            Some(v * 2 + (1 - self.round % 2))
        }
    }
}

/// Helper function to convert the arguments to a StableQuorumIterator into an ordered set of values.
pub fn stable_quorum_filter(seed: u64, round: u64, count: usize, overlap: u64) -> BTreeSet<usize> {
    StableQuorumIterator::new(seed, round, count as u64, overlap)
        // We should never have more than u32_max members in a test
        .map(|x| usize::try_from(x).unwrap())
        .collect()
}

/// Constructs a quorum with a random number of members and overlaps. Functions similar to StableQuorumIterator,
/// except that the number of MEMBERS and OVERLAP are also (deterministically) random, to allow additional variance
/// in testing.
pub struct RandomOverlapQuorumIterator {
    /// PRNG from the previous round
    prev_rng: NonRepeatValueIterator,

    /// PRNG for the current round
    this_rng: NonRepeatValueIterator,

    /// Current ROUND
    round: u64,

    /// Number of members to emit for the current round
    members: u64,

    /// OVERLAP of nodes to be carried over from the previous round
    overlap: u64,

    /// The next call to next() will emit the value with this index. Starts at 0 and is incremented for each
    /// call to next()
    index: u64,
}

impl RandomOverlapQuorumIterator {
    /// Create a new RandomOverlapQuorumIterator
    pub fn new(
        seed: u64,
        round: u64,
        count: u64,
        members_min: u64,
        members_max: u64,
        overlap_min: u64,
        overlap_max: u64,
    ) -> Self {
        assert!(
            members_min <= members_max,
            "Members_min cannot be greater than members_max"
        );
        assert!(
            overlap_min <= overlap_max,
            "Overlap_min cannot be greater than overlap_max"
        );
        assert!(
            overlap_max < members_min,
            "Overlap_max must be less than members_min"
        );
        assert!(
            count / 2 > overlap_max,
            "Overlap cannot be greater than the entire set size"
        );

        let (mut prev_rng, mut this_rng) = make_rngs(seed, round);

        // Consume two values from prev_rng to advance it to the same state it was at the beginning of the previous round
        let _prev_members = prev_rng.gen_range(members_min..=members_max);
        let _prev_overlap = prev_rng.gen_range(overlap_min..=overlap_max);
        let this_members = this_rng.gen_range(members_min..=members_max);
        let this_overlap = this_rng.gen_range(overlap_min..=overlap_max);

        Self {
            prev_rng: NonRepeatValueIterator::new(prev_rng, calc_num_slots(count, round % 2 == 0)),
            this_rng: NonRepeatValueIterator::new(this_rng, calc_num_slots(count, round % 2 == 1)),
            round,
            members: this_members,
            overlap: this_overlap,
            index: 0,
        }
    }
}

impl Iterator for RandomOverlapQuorumIterator {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.members {
            None
        } else if self.index < self.overlap {
            // Generate enough values for the previous round
            let v = self.prev_rng.next().unwrap();
            self.index += 1;
            Some(v * 2 + self.round % 2)
        } else {
            // Generate new values
            let v = self.this_rng.next().unwrap();
            self.index += 1;
            Some(v * 2 + (1 - self.round % 2))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stable() {
        for _ in 0..100 {
            let seed = rand::random::<u64>();
            let prev_set: Vec<u64> = StableQuorumIterator::new(seed, 1, 10, 2).collect();
            let this_set: Vec<u64> = StableQuorumIterator::new(seed, 2, 10, 2).collect();

            // The first two elements from prev_set are from its previous round. But its 2nd and 3rd elements
            // are new, and should be carried over to become the first two elements from this_set.
            assert_eq!(
                prev_set[2..4],
                this_set[0..2],
                "prev_set={prev_set:?}, this_set={this_set:?}"
            );
        }
    }

    #[test]
    fn test_random_overlap() {
        for _ in 0..100 {
            let seed = rand::random::<u64>();
            let prev_set: Vec<u64> =
                RandomOverlapQuorumIterator::new(seed, 1, 20, 5, 10, 2, 3).collect();
            let this_set: Vec<u64> =
                RandomOverlapQuorumIterator::new(seed, 2, 20, 5, 10, 2, 3).collect();

            // Similar to the overlap before, but there are 4 possible cases: the previous set might have had
            // either 2 or 3 overlaps, meaning we should start with index 2 or 3, and the overlap size might
            // be either 2 or 3. We'll just check for 2 overlaps, meaning we have two possible overlap cases
            // to verify.
            let matched = (prev_set[2..4] == this_set[0..2]) || (prev_set[3..5] == this_set[0..2]);
            assert!(matched, "prev_set={prev_set:?}, this_set={this_set:?}");
        }
    }
}
