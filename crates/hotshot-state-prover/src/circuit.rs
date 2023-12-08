//! Circuit implementation for verifying light client state update

use ark_ec::twisted_edwards::TECurveConfig;
use ark_ff::PrimeField;
use ark_std::borrow::Borrow;
use ethereum_types::U256;
use hotshot_stake_table::config::STAKE_TABLE_CAPACITY;
use hotshot_types::light_client::LightClientState;
use jf_plonk::errors::PlonkError;
use jf_primitives::{
    circuit::{
        rescue::RescueNativeGadget,
        signature::schnorr::{SignatureGadget, VerKeyVar},
    },
    rescue::RescueParameter,
    signatures::schnorr::{Signature, VerKey as SchnorrVerKey},
};
use jf_relation::{errors::CircuitError, Circuit, PlonkCircuit, Variable};

/// Lossy conversion of a U256 into a field element.
pub(crate) fn u256_to_field<F: PrimeField>(v: &U256) -> F {
    let mut bytes = vec![0u8; 32];
    v.to_little_endian(&mut bytes);
    F::from_le_bytes_mod_order(&bytes)
}

/// Variable for stake table entry
#[derive(Clone, Debug)]
pub struct StakeTableEntryVar {
    /// state verification keys
    pub state_ver_key: VerKeyVar,
    /// Stake amount
    pub stake_amount: Variable,
}

/// Light client state Variable
/// The stake table commitment is a triple (qc_keys_comm, state_keys_comm, stake_amount_comm).
/// Variable for a stake table commitment
#[derive(Clone, Debug)]
pub struct StakeTableCommVar {
    /// Commitment for QC verification keys
    pub qc_keys_comm: Variable,
    /// Commitment for state verification keys
    pub state_keys_comm: Variable,
    /// Commitment for stake amount
    pub stake_amount_comm: Variable,
}

/// Light client state Variable
#[derive(Clone, Debug)]
pub struct LightClientStateVar {
    /// Private list holding all variables
    ///  vars[0]: view number
    ///  vars[1]: block height
    ///  vars[2]: block commitment root
    ///  vars[3]: fee ledger commitment
    ///  vars[4-6]: stake table commitment
    vars: [Variable; 7],
}

#[derive(Clone, Debug)]
pub struct PublicInput<F: PrimeField>(Vec<F>);

impl<F: PrimeField> AsRef<[F]> for PublicInput<F> {
    fn as_ref(&self) -> &[F] {
        &self.0
    }
}

impl<F: PrimeField> From<Vec<F>> for PublicInput<F> {
    fn from(v: Vec<F>) -> Self {
        Self(v)
    }
}

impl<F: PrimeField> PublicInput<F> {
    /// Return the threshold
    pub fn threshold(&self) -> F {
        self.0[0]
    }

    /// Return the view number of the light client state
    pub fn view_number(&self) -> F {
        self.0[1]
    }

    /// Return the block height of the light client state
    pub fn block_height(&self) -> F {
        self.0[2]
    }

    /// Return the block commitment root of the light client state
    pub fn block_comm_root(&self) -> F {
        self.0[3]
    }

    /// Return the fee ledger commitment of the light client state
    pub fn fee_ledger_comm(&self) -> F {
        self.0[4]
    }

    /// Return the stake table commitment of the light client state
    pub fn stake_table_comm(&self) -> (F, F, F) {
        (self.0[5], self.0[6], self.0[7])
    }

    /// Return the qc key commitment of the light client state
    pub fn qc_key_comm(&self) -> F {
        self.0[5]
    }

    /// Return the state key commitment of the light client state
    pub fn state_key_comm(&self) -> F {
        self.0[6]
    }

    /// Return the stake amount commitment of the light client state
    pub fn stake_amount_comm(&self) -> F {
        self.0[7]
    }
}

impl LightClientStateVar {
    pub fn new<F: PrimeField>(
        circuit: &mut PlonkCircuit<F>,
        state: &LightClientState<F>,
    ) -> Result<Self, CircuitError> {
        let view_number_f = F::from(state.view_number as u64);
        let block_height_f = F::from(state.block_height as u64);
        Ok(Self {
            vars: [
                circuit.create_public_variable(view_number_f)?,
                circuit.create_public_variable(block_height_f)?,
                circuit.create_public_variable(state.block_comm_root)?,
                circuit.create_public_variable(state.fee_ledger_comm)?,
                circuit.create_public_variable(state.stake_table_comm.0)?,
                circuit.create_public_variable(state.stake_table_comm.1)?,
                circuit.create_public_variable(state.stake_table_comm.2)?,
            ],
        })
    }

    pub fn view_number(&self) -> Variable {
        self.vars[0]
    }

    pub fn block_height(&self) -> Variable {
        self.vars[1]
    }

    pub fn block_comm_root(&self) -> Variable {
        self.vars[2]
    }

    pub fn fee_ledger_comm(&self) -> Variable {
        self.vars[3]
    }

    pub fn stake_table_comm(&self) -> StakeTableCommVar {
        StakeTableCommVar {
            qc_keys_comm: self.vars[4],
            state_keys_comm: self.vars[5],
            stake_amount_comm: self.vars[6],
        }
    }
}

impl AsRef<[Variable]> for LightClientStateVar {
    fn as_ref(&self) -> &[Variable] {
        &self.vars
    }
}

/// A function that takes as input:
/// - a list of stake table entries (`Vec<(SchnorrVerKey, Amount)>`)
/// - a bit vector indicates the signers
/// - a list of schnorr signatures of the updated states (`Vec<SchnorrSignature>`), default if the node doesn't sign the state
/// - updated light client state (`(view_number, block_height, block_comm_root, fee_ledger_comm, stake_table_comm)`)
/// - a quorum threshold
/// Lengths of input vectors should not exceed the `STAKE_TABLE_CAPACITY`.
/// The list of stake table entries, bit indicators and signatures will be padded to the `STAKE_TABLE_CAPACITY`.
/// It checks that
/// - the signer's accumulated weight exceeds the quorum threshold
/// - the stake table corresponds to the one committed in the light client state
/// - all signed Schnorr signatures are valid
/// and returns
/// - A circuit for proof generation
/// - A list of public inputs for verification
/// - A PlonkError if any error happens when building the circuit
pub(crate) fn build<F, P, STIter, BitIter, SigIter>(
    stake_table_entries: STIter,
    signer_bit_vec: BitIter,
    signatures: SigIter,
    lightclient_state: &LightClientState<F>,
    threshold: &U256,
) -> Result<(PlonkCircuit<F>, PublicInput<F>), PlonkError>
where
    F: RescueParameter,
    P: TECurveConfig<BaseField = F>,
    STIter: IntoIterator,
    STIter::Item: Borrow<(SchnorrVerKey<P>, U256)>,
    STIter::IntoIter: ExactSizeIterator,
    BitIter: IntoIterator,
    BitIter::Item: Borrow<bool>,
    BitIter::IntoIter: ExactSizeIterator,
    SigIter: IntoIterator,
    SigIter::Item: Borrow<Signature<P>>,
    SigIter::IntoIter: ExactSizeIterator,
{
    let stake_table_entries = stake_table_entries.into_iter();
    let signer_bit_vec = signer_bit_vec.into_iter();
    let signatures = signatures.into_iter();
    if stake_table_entries.len() > STAKE_TABLE_CAPACITY {
        return Err(PlonkError::CircuitError(CircuitError::ParameterError(
            format!(
                "Number of input stake table entries {} exceeds the capacity {}",
                stake_table_entries.len(),
                STAKE_TABLE_CAPACITY,
            ),
        )));
    }
    if signer_bit_vec.len() > STAKE_TABLE_CAPACITY {
        return Err(PlonkError::CircuitError(CircuitError::ParameterError(
            format!(
                "Length of input bit vector {} exceeds the capacity {}",
                signer_bit_vec.len(),
                STAKE_TABLE_CAPACITY,
            ),
        )));
    }
    if signatures.len() > STAKE_TABLE_CAPACITY {
        return Err(PlonkError::CircuitError(CircuitError::ParameterError(
            format!(
                "Number of input signatures {} exceeds the capacity {}",
                signatures.len(),
                STAKE_TABLE_CAPACITY,
            ),
        )));
    }

    let mut circuit = PlonkCircuit::new_turbo_plonk();

    // creating variables for stake table entries
    let stake_table_entries_pad_len = STAKE_TABLE_CAPACITY - stake_table_entries.len();
    let mut stake_table_var = stake_table_entries
        .map(|item| {
            let item = item.borrow();
            let state_ver_key = circuit.create_signature_vk_variable(&item.0)?;
            let stake_amount = circuit.create_variable(u256_to_field::<F>(&item.1))?;
            Ok(StakeTableEntryVar {
                state_ver_key,
                stake_amount,
            })
        })
        .collect::<Result<Vec<_>, CircuitError>>()?;
    stake_table_var.extend(
        (0..stake_table_entries_pad_len)
            .map(|_| {
                let state_ver_key =
                    circuit.create_signature_vk_variable(&SchnorrVerKey::<P>::default())?;
                let stake_amount = circuit.create_variable(F::default())?;
                Ok(StakeTableEntryVar {
                    state_ver_key,
                    stake_amount,
                })
            })
            .collect::<Result<Vec<_>, CircuitError>>()?,
    );

    // creating variables for signatures
    let sig_pad_len = STAKE_TABLE_CAPACITY - signatures.len();
    let mut sig_vars = signatures
        .map(|sig| circuit.create_signature_variable(sig.borrow()))
        .collect::<Result<Vec<_>, CircuitError>>()?;
    sig_vars.extend(
        (0..sig_pad_len)
            .map(|_| circuit.create_signature_variable(&Signature::<P>::default()))
            .collect::<Result<Vec<_>, CircuitError>>()?,
    );

    // creating Boolean variables for the bit vector
    let bit_vec_pad_len = STAKE_TABLE_CAPACITY - signer_bit_vec.len();
    let mut signer_bit_vec_var = signer_bit_vec
        .map(|b| circuit.create_boolean_variable(*b.borrow()))
        .collect::<Result<Vec<_>, CircuitError>>()?;
    signer_bit_vec_var.extend(
        (0..bit_vec_pad_len)
            .map(|_| circuit.create_boolean_variable(false))
            .collect::<Result<Vec<_>, CircuitError>>()?,
    );

    let threshold = u256_to_field::<F>(threshold);
    let threshold_pub_var = circuit.create_public_variable(threshold)?;

    let lightclient_state_pub_var = LightClientStateVar::new(&mut circuit, lightclient_state)?;

    let view_number_f = F::from(lightclient_state.view_number as u64);
    let block_height_f = F::from(lightclient_state.block_height as u64);
    let public_inputs = vec![
        threshold,
        view_number_f,
        block_height_f,
        lightclient_state.block_comm_root,
        lightclient_state.fee_ledger_comm,
        lightclient_state.stake_table_comm.0,
        lightclient_state.stake_table_comm.1,
        lightclient_state.stake_table_comm.2,
    ];

    // Checking whether the accumulated weight exceeds the quorum threshold
    let mut signed_amount_var = (0..STAKE_TABLE_CAPACITY / 2)
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
    // Adding the last if STAKE_TABLE_CAPACITY is not a multiple of 2
    if STAKE_TABLE_CAPACITY % 2 == 1 {
        signed_amount_var.push(circuit.mul(
            stake_table_var[STAKE_TABLE_CAPACITY - 1].stake_amount,
            signer_bit_vec_var[STAKE_TABLE_CAPACITY - 1].0,
        )?);
    }
    let acc_amount_var = circuit.sum(&signed_amount_var)?;
    circuit.enforce_leq(threshold_pub_var, acc_amount_var)?;

    // checking the commitment for the list of schnorr keys
    let state_ver_key_preimage_vars = stake_table_var
        .iter()
        .flat_map(|var| [var.state_ver_key.0.get_x(), var.state_ver_key.0.get_y()])
        .collect::<Vec<_>>();
    let state_ver_key_comm = RescueNativeGadget::<F>::rescue_sponge_with_padding(
        &mut circuit,
        &state_ver_key_preimage_vars,
        1,
    )?[0];
    circuit.enforce_equal(
        state_ver_key_comm,
        lightclient_state_pub_var.stake_table_comm().state_keys_comm,
    )?;

    // checking the commitment for the list of stake amounts
    let stake_amount_preimage_vars = stake_table_var
        .iter()
        .map(|var| var.stake_amount)
        .collect::<Vec<_>>();
    let stake_amount_comm = RescueNativeGadget::<F>::rescue_sponge_with_padding(
        &mut circuit,
        &stake_amount_preimage_vars,
        1,
    )?[0];
    circuit.enforce_equal(
        stake_amount_comm,
        lightclient_state_pub_var
            .stake_table_comm()
            .stake_amount_comm,
    )?;

    // checking all signatures
    let verification_result_vars = stake_table_var
        .iter()
        .zip(sig_vars)
        .map(|(entry, sig)| {
            SignatureGadget::<_, P>::check_signature_validity(
                &mut circuit,
                &entry.state_ver_key,
                lightclient_state_pub_var.as_ref(),
                &sig,
            )
        })
        .collect::<Result<Vec<_>, CircuitError>>()?;
    let bit_x_result_vars = signer_bit_vec_var
        .iter()
        .zip(verification_result_vars)
        .map(|(&bit, result)| {
            let neg_bit = circuit.logic_neg(bit)?;
            circuit.logic_or(neg_bit, result)
        })
        .collect::<Result<Vec<_>, CircuitError>>()?;
    let sig_ver_result = circuit.logic_and_all(&bit_x_result_vars)?;
    circuit.enforce_true(sig_ver_result.0)?;

    circuit.finalize_for_arithmetization()?;
    Ok((circuit, public_inputs.into()))
}

/// Internal function to build a dummy circuit
pub(crate) fn build_for_preprocessing<F, P>(
) -> Result<(PlonkCircuit<F>, PublicInput<F>), PlonkError>
where
    F: RescueParameter,
    P: TECurveConfig<BaseField = F>,
{
    let lightclient_state = LightClientState {
        view_number: 0,
        block_height: 0,
        block_comm_root: F::default(),
        fee_ledger_comm: F::default(),
        stake_table_comm: (F::default(), F::default(), F::default()),
    };
    build::<F, P, _, _, _>(&[], &[], &[], &lightclient_state, &U256::zero())
}

#[cfg(test)]
mod tests {
    use super::{build, LightClientState};
    use crate::utils::{key_pairs_for_testing, stake_table_for_testing};
    use ark_ed_on_bn254::EdwardsConfig as Config;
    use ethereum_types::U256;
    use hotshot_types::traits::stake_table::{SnapshotVersion, StakeTableScheme};
    use jf_primitives::{
        crhf::{VariableLengthRescueCRHF, CRHF},
        errors::PrimitivesError,
        signatures::{schnorr::Signature, SchnorrSignatureScheme, SignatureScheme},
    };
    use jf_relation::Circuit;
    use jf_utils::test_rng;

    type F = ark_ed_on_bn254::Fq;

    #[test]
    fn crypto_test_circuit_building() {
        let num_validators = 10;
        let mut prng = test_rng();

        let (qc_keys, state_keys) = key_pairs_for_testing(num_validators, &mut prng);
        let st = stake_table_for_testing(&qc_keys, &state_keys);

        let entries = st
            .try_iter(SnapshotVersion::LastEpochStart)
            .unwrap()
            .map(|(_, stake_amount, state_key)| (state_key, stake_amount))
            .collect::<Vec<_>>();

        let block_comm_root =
            VariableLengthRescueCRHF::<F, 1>::evaluate(vec![F::from(1u32), F::from(2u32)]).unwrap()
                [0];
        let fee_ledger_comm =
            VariableLengthRescueCRHF::<F, 1>::evaluate(vec![F::from(3u32), F::from(5u32)]).unwrap()
                [0];

        let lightclient_state = LightClientState {
            view_number: 100,
            block_height: 73,
            block_comm_root,
            fee_ledger_comm,
            stake_table_comm: st.commitment(SnapshotVersion::LastEpochStart).unwrap(),
        };
        let state_msg: [F; 7] = lightclient_state.clone().into();

        let sigs = state_keys
            .iter()
            .map(|(key, _)| SchnorrSignatureScheme::<Config>::sign(&(), key, state_msg, &mut prng))
            .collect::<Result<Vec<_>, PrimitivesError>>()
            .unwrap();

        // bit vector with total weight 26
        let bit_vec = [
            true, true, true, false, true, true, false, false, true, false,
        ];
        let bit_masked_sigs = bit_vec
            .iter()
            .zip(sigs.iter())
            .map(|(bit, sig)| {
                if *bit {
                    sig.clone()
                } else {
                    Signature::<Config>::default()
                }
            })
            .collect::<Vec<_>>();
        // good path
        let (circuit, public_inputs) = build(
            &entries,
            &bit_vec,
            &bit_masked_sigs,
            &lightclient_state,
            &U256::from(26u32),
        )
        .unwrap();
        assert!(circuit
            .check_circuit_satisfiability(public_inputs.as_ref())
            .is_ok());

        let (circuit, public_inputs) = build(
            &entries,
            &bit_vec,
            &bit_masked_sigs,
            &lightclient_state,
            &U256::from(10u32),
        )
        .unwrap();
        assert!(circuit
            .check_circuit_satisfiability(public_inputs.as_ref())
            .is_ok());

        // bad path: total weight doesn't meet the threshold
        // bit vector with total weight 23
        let bad_bit_vec = [
            true, true, true, true, true, false, false, true, false, false,
        ];
        let bad_bit_masked_sigs = bad_bit_vec
            .iter()
            .zip(sigs.iter())
            .map(|(bit, sig)| {
                if *bit {
                    sig.clone()
                } else {
                    Signature::<Config>::default()
                }
            })
            .collect::<Vec<_>>();
        let (bad_circuit, public_inputs) = build(
            &entries,
            &bad_bit_vec,
            &bad_bit_masked_sigs,
            &lightclient_state,
            &U256::from(25u32),
        )
        .unwrap();
        assert!(bad_circuit
            .check_circuit_satisfiability(public_inputs.as_ref())
            .is_err());

        // bad path: bad stake table commitment
        let mut bad_lightclient_state = lightclient_state.clone();
        bad_lightclient_state.stake_table_comm.1 = F::default();
        let bad_state_msg: [F; 7] = bad_lightclient_state.clone().into();
        let sig_for_bad_state = state_keys
            .iter()
            .map(|(key, _)| {
                SchnorrSignatureScheme::<Config>::sign(&(), key, bad_state_msg, &mut prng)
            })
            .collect::<Result<Vec<_>, PrimitivesError>>()
            .unwrap();
        let (bad_circuit, public_inputs) = build(
            &entries,
            &bit_vec,
            &sig_for_bad_state,
            &bad_lightclient_state,
            &U256::from(26u32),
        )
        .unwrap();
        assert!(bad_circuit
            .check_circuit_satisfiability(public_inputs.as_ref())
            .is_err());

        // bad path: incorrect signatures
        let mut wrong_light_client_state = lightclient_state.clone();
        // state with a different qc key commitment
        wrong_light_client_state.stake_table_comm.0 = F::default();
        let wrong_state_msg: [F; 7] = wrong_light_client_state.into();
        let wrong_sigs = state_keys
            .iter()
            .map(|(key, _)| {
                SchnorrSignatureScheme::<Config>::sign(&(), key, wrong_state_msg, &mut prng)
            })
            .collect::<Result<Vec<_>, PrimitivesError>>()
            .unwrap();
        let (bad_circuit, public_inputs) = build(
            &entries,
            &bit_vec,
            &wrong_sigs,
            &lightclient_state,
            &U256::from(26u32),
        )
        .unwrap();
        assert!(bad_circuit
            .check_circuit_satisfiability(public_inputs.as_ref())
            .is_err());
    }
}
