//! Types for verification circuit

use ark_ff::PrimeField;
pub use jf_primitives::circuit::signature::schnorr::{SignatureVar, VerKeyVar};
use jf_relation::Variable;
use serde::{Deserialize, Serialize};

/// Variable for stake table entry
#[derive(Clone, Debug)]
pub struct StakeTableEntryVar {
    pub bls_ver_key: (Variable, Variable),
    pub schnorr_ver_key: VerKeyVar,
    pub stake_amount: Variable,
}

/// HotShot state Variable
#[derive(Clone, Debug)]
pub struct HotShotStateVar {
    pub view_number_var: Variable,
    pub block_height_var: Variable,
    pub block_comm_var: Variable,
    pub fee_ledger_comm_var: Variable,
    pub stake_table_comm_var: Variable,
}

/// HotShot state
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HotShotState<F: PrimeField> {
    pub view_number: usize,
    pub block_height: usize,
    pub block_comm: F,
    pub fee_ledger_comm: F,
    pub stake_table_comm: F,
}

///
pub trait IntoFields<F: PrimeField> {
    const LEN: usize;
    fn into_fields() -> Vec<F>;
}
