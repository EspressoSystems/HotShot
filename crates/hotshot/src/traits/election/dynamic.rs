// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::hash::{DefaultHasher, Hash, Hasher};

use anyhow::{Context, Result};
use hotshot_types::traits::{node_implementation::NodeType, signature_key::SignatureKey};
use sha2::{Digest, Sha256};

// TODO: Add the following consts once we bench the hash time.
// <https://github.com/EspressoSystems/HotShot/issues/3880>
// /// Highest number of hashes that a hardware can complete in a second.
// const `HASHES_PER_SECOND`
// /// Time a DRB calculation will take, in terms of number of views.
// const `DRB_CALCULATION_NUM_VIEW`: u64 = 300;

// TODO: Replace this with an accurate number calculated by `fn difficulty_level()` once we bench
// the hash time.
// <https://github.com/EspressoSystems/HotShot/issues/3880>
/// Arbitrary number of times the hash function will be repeatedly called.
const DIFFICULTY_LEVEL: u64 = 10;

// TODO: Use `HASHES_PER_SECOND` * `VIEW_TIMEOUT` * `DRB_CALCULATION_NUM_VIEW` to calculate this
// once we bench the hash time.
// <https://github.com/EspressoSystems/HotShot/issues/3880>
/// Difficulty level of the DRB calculation.
///
/// Represents the number of times the hash function will be repeatedly called.
#[must_use]
pub fn difficulty_level() -> u64 {
    unimplemented!("Use an arbitrary `DIFFICULTY_LEVEL` for now before we bench the hash time.");
}

// TODO: Replace `hashes_per_second` parameter with a const once we bench the hash time.
// <https://github.com/EspressoSystems/HotShot/issues/3880>
/// Compute the DRB seed result for the leader rotation.
///
/// This is to be started two epochs in advance and spawned in a non-blocking thread.
///
/// # Errors
/// * If failed to serialize the QC signature.
pub fn compute_drb_seed<TYPES: NodeType>(
    qc_signature: &<TYPES::SignatureKey as SignatureKey>::QcType,
) -> Result<[u8; 32]> {
    // Hash the QC signature.
    let mut hash = bincode::serialize(&qc_signature)
        .with_context(|| "Failed to serialize the QC signature.")?;
    for _iter in 0..DIFFICULTY_LEVEL {
        // TODO: This may be optimized to avoid memcopies after we bench the hash time.
        // <https://github.com/EspressoSystems/HotShot/issues/3880>
        hash = Sha256::digest(hash).to_vec();
    }

    // Convert the hash to a seed as the DRB result.
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&hash);
    Ok(seed)
}

/// Use the DRB seed result to get the leader.
#[must_use]
pub fn leader<TYPES: NodeType>(
    view_number: usize,
    stake_table: &[<TYPES::SignatureKey as SignatureKey>::StakeTableEntry],
    drb_seed: [u8; 32],
) -> TYPES::SignatureKey {
    let mut hasher = DefaultHasher::new();
    drb_seed.hash(&mut hasher);
    view_number.hash(&mut hasher);
    #[allow(clippy::cast_possible_truncation)]
    let index = (hasher.finish() as usize) % stake_table.len();
    // TODO: Index with a weighted stake table.
    // <https://github.com/EspressoSystems/HotShot/issues/3898>
    let entry = stake_table[index].clone();
    TYPES::SignatureKey::public_key(&entry)
}
