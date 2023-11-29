//! Implementations for examples and tests only
use commit::{Commitment, Committable};
use derivative::Derivative;

use hotshot::traits::election::static_committee::{GeneralStaticCommittee, StaticElectionConfig};

use hotshot_signature_key::bn254::BLSPubKey;
use hotshot_types::{
    data::{fake_commitment, BlockError, ViewNumber},
    traits::{
        election::Membership,
        node_implementation::NodeType,
        state::{ConsensusTime, TestableState},
        BlockPayload, State,
    },
};

use serde::{Deserialize, Serialize};
use std::{fmt::Debug, marker::PhantomData};

use crate::demo::block::VIDTransaction;

use self::block::{VIDBlockHeader, VIDBlockPayload};

/// This module provides implementations of block traits for examples and tests only.
pub mod block {
    use std::{
        fmt::{Debug, Display},
        mem::size_of,
    };

    use ark_serialize::CanonicalDeserialize;
    use commit::{Commitment, Committable};
    use hotshot_types::{
        data::{BlockError, VidCommitment, VidScheme, VidSchemeTrait},
        traits::{
            block_contents::{vid_commitment, BlockHeader, Transaction},
            state::TestableBlock,
            BlockPayload,
        },
    };
    use serde::{Deserialize, Serialize};
    use sha3::{Digest, Keccak256};

    /// The transaction in a [`VIDBlockPayload`].
    #[derive(Default, PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
    pub struct VIDTransaction(pub Vec<u8>);

    impl VIDTransaction {
        /// Encode a list of transactions into bytes.
        ///
        /// # Errors
        /// If the transaction length conversion fails.
        pub fn encode(transactions: Vec<Self>) -> Result<Vec<u8>, BlockError> {
            let mut encoded = Vec::new();

            for txn in transactions {
                // The transaction length is converted from `usize` to `u32` to ensure consistent
                // number of bytes on different platforms.
                let txn_size = match u32::try_from(txn.0.len()) {
                    Ok(len) => len.to_le_bytes(),
                    Err(_) => {
                        return Err(BlockError::InvalidTransactionLength);
                    }
                };

                // Concatenate the bytes of the transaction size and the transaction itself.
                encoded.extend(txn_size);
                encoded.extend(txn.0);
            }

            Ok(encoded)
        }
    }

    impl Committable for VIDTransaction {
        fn commit(&self) -> Commitment<Self> {
            let builder = commit::RawCommitmentBuilder::new("Txn Comm");
            let mut hasher = Keccak256::new();
            hasher.update(&self.0);
            let generic_array = hasher.finalize();
            builder.generic_byte_array(&generic_array).finalize()
        }

        fn tag() -> String {
            "SEQUENCING_TXN".to_string()
        }
    }

    impl Transaction for VIDTransaction {}

    /// A [`BlockPayload`] that contains a list of `VIDTransaction`.
    #[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
    pub struct VIDBlockPayload {
        /// List of transactions.
        pub transactions: Vec<VIDTransaction>,
        /// VID commitment to the block payload.
        pub payload_commitment: <VidScheme as VidSchemeTrait>::Commit,
    }

    impl VIDBlockPayload {
        /// Create a genesis block payload with transaction bytes `vec![0]`, to be used for
        /// consensus task initiation.
        /// # Panics
        /// If the `VidScheme` construction fails.
        #[must_use]
        pub fn genesis() -> Self {
            let txns: Vec<u8> = vec![0];
            // It's impossible for `encode` to fail because the transaciton length is very small.
            let encoded = VIDTransaction::encode(vec![VIDTransaction(txns.clone())]).unwrap();
            VIDBlockPayload {
                transactions: vec![VIDTransaction(txns)],
                payload_commitment: vid_commitment(&encoded),
            }
        }
    }

    impl Committable for VIDBlockPayload {
        fn commit(&self) -> Commitment<Self> {
            <Commitment<Self> as CanonicalDeserialize>::deserialize(&*self.payload_commitment)
                .expect("conversion from VidScheme::Commit to Commitment should succeed")
        }

        fn tag() -> String {
            "VID_BLOCK_PAYLOAD".to_string()
        }
    }

    impl Display for VIDBlockPayload {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "BlockPayload #txns={}", self.transactions.len())
        }
    }

    impl TestableBlock for VIDBlockPayload {
        fn genesis() -> Self {
            Self::genesis()
        }

        fn txn_count(&self) -> u64 {
            self.transactions.len() as u64
        }
    }

    impl BlockPayload for VIDBlockPayload {
        type Error = BlockError;
        type Transaction = VIDTransaction;
        type Metadata = ();
        type Encode<'a> = <Vec<u8> as IntoIterator>::IntoIter;

        fn from_transactions(
            transactions: impl IntoIterator<Item = Self::Transaction>,
        ) -> Result<(Self, Self::Metadata), Self::Error> {
            let txns_vec: Vec<VIDTransaction> = transactions.into_iter().collect();
            let encoded = VIDTransaction::encode(txns_vec.clone())?;
            Ok((
                Self {
                    transactions: txns_vec,
                    payload_commitment: vid_commitment(&encoded),
                },
                (),
            ))
        }

        fn from_bytes<E>(encoded_transactions: E, _metadata: Self::Metadata) -> Self
        where
            E: Iterator<Item = u8>,
        {
            let encoded_vec: Vec<u8> = encoded_transactions.collect();
            let mut transactions = Vec::new();
            let mut current_index = 0;
            while current_index < encoded_vec.len() {
                // Decode the transaction length.
                let txn_start_index = current_index + size_of::<u32>();
                let mut txn_len_bytes = [0; size_of::<u32>()];
                txn_len_bytes.copy_from_slice(&encoded_vec[current_index..txn_start_index]);
                let txn_len: usize = u32::from_le_bytes(txn_len_bytes) as usize;

                // Get the transaction.
                let next_index = txn_start_index + txn_len;
                transactions.push(VIDTransaction(
                    encoded_vec[txn_start_index..next_index].to_vec(),
                ));
                current_index = next_index;
            }

            Self {
                transactions,
                payload_commitment: vid_commitment(&encoded_vec),
            }
        }

        fn genesis() -> (Self, Self::Metadata) {
            (Self::genesis(), ())
        }

        fn encode(&self) -> Result<Self::Encode<'_>, Self::Error> {
            Ok(VIDTransaction::encode(self.transactions.clone())?.into_iter())
        }

        fn transaction_commitments(&self) -> Vec<Commitment<Self::Transaction>> {
            self.transactions
                .iter()
                .map(commit::Committable::commit)
                .collect()
        }
    }

    /// A [`BlockHeader`] that commits to [`VIDBlockPayload`].
    #[derive(PartialEq, Eq, Hash, Clone, Debug, Deserialize, Serialize)]
    pub struct VIDBlockHeader {
        /// Block number.
        pub block_number: u64,
        /// VID commitment to the payload.
        pub payload_commitment: VidCommitment,
    }

    impl BlockHeader for VIDBlockHeader {
        type Payload = VIDBlockPayload;

        fn new(
            payload_commitment: VidCommitment,
            _metadata: <Self::Payload as BlockPayload>::Metadata,
            parent_header: &Self,
        ) -> Self {
            Self {
                block_number: parent_header.block_number + 1,
                payload_commitment,
            }
        }

        fn genesis() -> (
            Self,
            Self::Payload,
            <Self::Payload as BlockPayload>::Metadata,
        ) {
            let (payload, metadata) = <Self::Payload as BlockPayload>::genesis();
            (
                Self {
                    block_number: 0,
                    payload_commitment: payload.payload_commitment,
                },
                payload,
                metadata,
            )
        }

        fn block_number(&self) -> u64 {
            self.block_number
        }

        fn payload_commitment(&self) -> VidCommitment {
            self.payload_commitment
        }

        fn metadata(&self) -> <Self::Payload as BlockPayload>::Metadata {}
    }
}

/// Dummy implementation of `State` for unit tests
pub mod state {
    use super::block::{VIDBlockHeader, VIDBlockPayload, VIDTransaction};
    use commit::Committable;
    use espresso_systems_common::hotshot::tag;
    use hotshot_types::data::ViewNumber;
    use hotshot_types::traits::state::{State, TestableState};
    use rand::Rng;
    use serde::{Deserialize, Serialize};
    use std::fmt::Debug;
    use std::hash::Hash;

    /// Dummy error
    #[derive(Debug)]
    pub struct DummyError;

    impl std::error::Error for DummyError {}

    impl std::fmt::Display for DummyError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("A bad thing happened")
        }
    }

    /// The dummy state
    #[derive(Clone, Debug, Default, Hash, PartialEq, Eq, Serialize, Deserialize)]
    pub struct DummyState {
        /// Some dummy data
        nonce: u64,
    }

    impl Committable for DummyState {
        fn commit(&self) -> commit::Commitment<Self> {
            commit::RawCommitmentBuilder::new("Dummy State Comm")
                .u64_field("Nonce", self.nonce)
                .finalize()
        }

        fn tag() -> String {
            tag::DUMMY_STATE.to_string()
        }
    }

    impl DummyState {
        /// Generate a random `DummyState`
        pub fn random(r: &mut dyn rand::RngCore) -> Self {
            Self {
                nonce: r.gen_range(1..1_000_000),
            }
        }
    }

    impl State for DummyState {
        type Error = DummyError;
        type BlockHeader = VIDBlockHeader;
        type BlockPayload = VIDBlockPayload;
        type Time = ViewNumber;

        fn validate_block(
            &self,
            _block_header: &Self::BlockHeader,
            _view_number: &Self::Time,
        ) -> bool {
            false
        }

        fn initialize() -> Self {
            let mut state = Self::default();
            state.nonce += 1;
            state
        }

        fn append(
            &self,
            _block_header: &Self::BlockHeader,
            _view_number: &Self::Time,
        ) -> Result<Self, Self::Error> {
            Ok(Self {
                nonce: self.nonce + 1,
            })
        }

        fn on_commit(&self) {}
    }

    impl TestableState for DummyState {
        fn create_random_transaction(
            _state: Option<&Self>,
            _: &mut dyn rand::RngCore,
            _: u64,
        ) -> VIDTransaction {
            VIDTransaction(vec![0u8])
        }
    }
}

/// sequencing demo entry state
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct DemoState {
    /// the block height
    block_height: u64,
    /// the view number
    view_number: ViewNumber,
    /// the previous state commitment
    prev_state_commitment: Commitment<Self>,
}

impl Committable for DemoState {
    fn commit(&self) -> Commitment<Self> {
        commit::RawCommitmentBuilder::new("Demo State Commit")
            .u64_field("block_height", self.block_height)
            .u64_field("view_number", *self.view_number)
            .field("prev_state_commitment", self.prev_state_commitment)
            .finalize()
    }

    fn tag() -> String {
        "SEQUENCING_DEMO_STATE".to_string()
    }
}

impl Default for DemoState {
    fn default() -> Self {
        Self {
            block_height: 0,
            view_number: ViewNumber::genesis(),
            prev_state_commitment: fake_commitment(),
        }
    }
}

impl State for DemoState {
    type Error = BlockError;

    type BlockHeader = VIDBlockHeader;

    type BlockPayload = VIDBlockPayload;

    type Time = ViewNumber;

    fn validate_block(&self, _block_header: &Self::BlockHeader, view_number: &Self::Time) -> bool {
        if view_number == &ViewNumber::genesis() {
            &self.view_number == view_number
        } else {
            self.view_number < *view_number
        }
    }

    fn initialize() -> Self {
        let mut state = Self::default();
        state.block_height += 1;
        state
    }

    fn append(
        &self,
        block_header: &Self::BlockHeader,
        view_number: &Self::Time,
    ) -> Result<Self, Self::Error> {
        if !self.validate_block(block_header, view_number) {
            return Err(BlockError::InvalidBlockHeader);
        }

        Ok(DemoState {
            block_height: self.block_height + 1,
            view_number: *view_number,
            prev_state_commitment: self.commit(),
        })
    }

    fn on_commit(&self) {}
}

impl TestableState for DemoState {
    fn create_random_transaction(
        _state: Option<&Self>,
        _rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <Self::BlockPayload as BlockPayload>::Transaction {
        /// clippy appeasement for `RANDOM_TX_BASE_SIZE`
        const RANDOM_TX_BASE_SIZE: usize = 8;
        VIDTransaction(vec![0; RANDOM_TX_BASE_SIZE + (padding as usize)])
    }
}
/// Implementation of [`NodeType`] for [`VDemoNode`]
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub struct DemoTypes;

impl NodeType for DemoTypes {
    type Time = ViewNumber;
    type BlockHeader = VIDBlockHeader;
    type BlockPayload = VIDBlockPayload;
    type SignatureKey = BLSPubKey;
    type Transaction = VIDTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = DemoState;
    type Membership = DemoMembership;
}

/// Alias for the static committee used in the Demo apps
pub type DemoMembership = GeneralStaticCommittee<DemoTypes, <DemoTypes as NodeType>::SignatureKey>;

/// The node implementation for the sequencing demo
#[derive(Derivative)]
#[derivative(Clone(bound = ""))]
pub struct DemoNode<MEMBERSHIP>(PhantomData<MEMBERSHIP>)
where
    MEMBERSHIP: Membership<DemoTypes> + std::fmt::Debug;

impl<MEMBERSHIP> DemoNode<MEMBERSHIP>
where
    MEMBERSHIP: Membership<DemoTypes> + std::fmt::Debug,
{
    /// Create a new `DemoNode`
    #[must_use]
    pub fn new() -> Self {
        DemoNode(PhantomData)
    }
}

impl<MEMBERSHIP> Debug for DemoNode<MEMBERSHIP>
where
    MEMBERSHIP: Membership<DemoTypes> + std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DemoNode")
            .field("_phantom", &"phantom")
            .finish()
    }
}

impl<MEMBERSHIP> Default for DemoNode<MEMBERSHIP>
where
    MEMBERSHIP: Membership<DemoTypes> + std::fmt::Debug,
{
    fn default() -> Self {
        Self::new()
    }
}
