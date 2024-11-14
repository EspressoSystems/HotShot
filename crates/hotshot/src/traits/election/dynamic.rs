// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use anyhow::{Context, Result};
use hotshot_types::traits::{node_implementation::NodeType, signature_key::SignatureKey};
use sha2::{Digest, Sha256};

/// Time a DRB calculation will take, in terms of number of views.
const DRB_CALCULATION_NUM_VIEW: u64 = 300;

// TODO: Replace `hashes_per_second` parameter with a const once we benchmark the hash time.
// <https://github.com/EspressoSystems/HotShot/issues/3880>
/// Difficulty level of the DRB calculation.
///
/// Represents the number of times the hash function will be repeatedly called.
///
/// # Arguments
/// * `hashes_per_second` - Number of hashes that can be completed in a second.
/// * `view_timeout` - Timeout duration for a view, in seconds.
#[must_use]
pub fn difficulty_level(hashes_per_second: u64, view_timeout: u64) -> u64 {
    hashes_per_second * view_timeout * DRB_CALCULATION_NUM_VIEW
}

// TODO: Replace `hashes_per_second` parameter with a const once we benchmark the hash time.
// <https://github.com/EspressoSystems/HotShot/issues/3880>
/// Compute the DRB result for the leader rotation.
///
/// This is to be started two epochs in advance.
///
/// # Arguments
/// * `qc_signature` - Aggregated signature in the current quorum proposal.
/// * `view_timeout` - Timeout duration for a view, in seconds.
///
/// # Errors
/// * If failed to serialize the QC signature.
pub fn compute_drb<TYPES: NodeType>(
    qc_signature: &<TYPES::SignatureKey as SignatureKey>::QcType,
    hashes_per_second: u64,
    view_timeout: u64,
) -> Result<usize> {
    // Hash the QC signature.
    let mut hash = bincode::serialize(&qc_signature)
        .with_context(|| "Failed to serialize the QC signature.")?;
    let difficulty_level = difficulty_level(hashes_per_second, view_timeout);
    for _iter in 0..difficulty_level {
        hash = Sha256::digest(hash).to_vec();
    }

    // Convert the hash to a seed as the DRB result.
    let mut seed = [0u8; 8];
    seed.copy_from_slice(&hash[..size_of::<usize>()]);
    Ok(usize::from_le_bytes(seed))
}

/// Use the DRB result to get the leader.
#[must_use]
pub fn leader<TYPES: NodeType>(
    stake_table: &[<TYPES::SignatureKey as SignatureKey>::StakeTableEntry],
    drb: usize,
) -> TYPES::SignatureKey {
    let index = drb % stake_table.len();
    let res = stake_table[index].clone();
    TYPES::SignatureKey::public_key(&res)
}
