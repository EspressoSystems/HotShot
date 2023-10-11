//! Circuit implementation for verifying light client state update

use std::marker::PhantomData;

use ark_ec::twisted_edwards::TECurveConfig;
use ark_ff::PrimeField;
use ethereum_types::U256;
use hotshot_types::traits::stake_table::{SnapshotVersion, StakeTableScheme};
use jf_plonk::errors::PlonkError;
use jf_primitives::{
    circuit::signature::schnorr::VerKeyVar,
    rescue::RescueParameter,
    signatures::{
        bls_over_bn254::VerKey as BLSVerKey,
        schnorr::{Signature, VerKey as SchnorrVerKey},
    },
};
use jf_relation::{
    errors::CircuitError, gadgets::ecc::SWToTEConParam, BoolVar, Circuit, PlonkCircuit,
};
use jf_relation::{gadgets::ecc::TEPoint, Variable};
use serde::{Deserialize, Serialize};

/// Number of entries/keys in the stake table
pub const NUM_ENTRIES: usize = 1000;

/// convert a U256 to a field element.
pub(crate) fn u256_to_field<F: PrimeField>(v: &U256) -> F {
    let mut bytes = vec![0u8; 32];
    v.to_little_endian(&mut bytes);
    F::from_le_bytes_mod_order(&bytes)
}

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

#[derive(Clone, Debug)]
pub struct StateUpdateBuilder<F: RescueParameter>(PhantomData<F>);

impl<F> StateUpdateBuilder<F>
where
    F: RescueParameter + SWToTEConParam,
{
    /// A function that takes as input:
    /// - stake table entries (`Vec<(BLSVerKey, SchnorrVerKey, Amount)>`)
    /// - schnorr signatures of the updated states (`Vec<SchnorrSignature>`)
    /// - updated hotshot state (`(view_number, block_height, block_comm, fee_ledger_comm, stake_table_comm)`)
    /// - signer bit vector
    /// - quorum threshold
    /// checks that
    /// - the signer's accumulated weight exceeds the quorum threshold
    /// - the commitment of the stake table
    /// - all schnorr signatures are valid
    pub fn build<ST, P>(
        stake_table: &ST,
        _sigs: &[Signature<P>],
        _hotshot_state: &HotShotState<F>,
        signer_bit_vec: &[bool],
        threshold: &U256,
    ) -> Result<PlonkCircuit<F>, PlonkError>
    where
        ST: StakeTableScheme<Key = (BLSVerKey, SchnorrVerKey<P>), Amount = U256>,
        P: TECurveConfig<BaseField = F>,
    {
        let mut circuit = PlonkCircuit::new_turbo_plonk();

        // Dummy circuit implementation, fill in the details later
        // TODO(Chengyu):
        // - [DONE] the signer's accumulated weight exceeds the quorum threshold
        // - The commitment of the stake table as [https://www.notion.so/espressosys/Light-Client-Contract-a416ebbfa9f342d79fccbf90de9706ef?pvs=4#6c0e26d753cd42e9bb0f22db1c519f45]
        // - Batch Schnorr signature verification

        // creating variables for stake table entries
        let mut stake_table_var = stake_table
            .try_iter(SnapshotVersion::LastEpochStart)?
            .map(|((_bls_ver_key, schnorr_ver_key), amount)| {
                // TODO(Chengyu): create variable for bls_key_var
                let schnorr_ver_key =
                    VerKeyVar(circuit.create_point_variable(schnorr_ver_key.to_affine().into())?);
                let stake_amount = circuit.create_variable(u256_to_field::<F>(&amount))?;
                Ok(StakeTableEntryVar {
                    bls_ver_key: (0, 0), // TODO(Chengyu)
                    schnorr_ver_key,
                    stake_amount,
                })
            })
            .collect::<Result<Vec<_>, CircuitError>>()?;
        let dummy_ver_key_var =
            VerKeyVar(circuit.create_constant_point_variable(TEPoint::default())?);
        stake_table_var.resize(
            NUM_ENTRIES,
            StakeTableEntryVar {
                bls_ver_key: (0, 0),
                schnorr_ver_key: dummy_ver_key_var,
                stake_amount: 0,
            },
        );

        let mut signer_bit_vec_var = signer_bit_vec
            .iter()
            .map(|&b| circuit.create_boolean_variable(b))
            .collect::<Result<Vec<_>, CircuitError>>()?;
        signer_bit_vec_var.resize(NUM_ENTRIES, BoolVar(circuit.zero()));

        let threshold_var = circuit.create_public_variable(u256_to_field::<F>(threshold))?;

        // Checking whether the accumulated weight exceeds the quorum threshold
        // We assume that NUM_ENTRIES is always a multiple of 2
        let signed_amount_var = (0..NUM_ENTRIES / 2)
            .map(|i| {
                circuit.mul_add(
                    &[
                        stake_table_var[2 * i].stake_amount,
                        signer_bit_vec_var[2 * i].0,
                        stake_table_var[2 * i + 1].stake_amount,
                        signer_bit_vec_var[2 * i + 1].0,
                    ],
                    &[F::one(), F::one()],
                )
            })
            .collect::<Result<Vec<_>, CircuitError>>()?;
        let acc_amount_var = circuit.sum(&signed_amount_var)?;
        circuit.enforce_leq(threshold_var, acc_amount_var)?;

        // circuit.mul_add(wires_in, q_muls)
        circuit.finalize_for_arithmetization()?;
        Ok(circuit)
    }
}

// #[cfg(test)]
// mod tests {
//     use ark_ed_on_bn254::EdwardsConfig as Config;
//     use hotshot_stake_table::mt_based::StakeTable;
//     use jf_primitives::{
//         rescue::RescueParameter,
//         signatures::{
//             bls_over_bn254::VerKey as BLSVerKey,
//             schnorr::{Signature, VerKey as SchnorrVerKey},
//         },
//     };

//     type Key = (BLSVerKey, SchnorrVerKey<Config>);
//     #[test]
//     fn test_circuit_building() {
//         let stake_table = StakeTable::<Key>::new(10);
//     }
// }
