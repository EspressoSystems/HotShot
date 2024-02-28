use std::{
    fmt::{Debug, Display},
    mem::size_of,
};

use commit::{Commitment, Committable, RawCommitmentBuilder};
use hotshot_types::{
    data::BlockError,
    traits::{
        block_contents::{BlockHeader, TestableBlock, Transaction},
        BlockPayload, ValidatedState,
    },
    utils::BuilderCommitment,
    vid::VidCommitment,
};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};

use crate::state_types::TestValidatedState;

/// The transaction in a [`TestBlockPayload`].
#[derive(Default, PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct TestTransaction(pub Vec<u8>);

impl TestTransaction {
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

impl Committable for TestTransaction {
    fn commit(&self) -> Commitment<Self> {
        let builder = commit::RawCommitmentBuilder::new("Txn Comm");
        let mut hasher = Keccak256::new();
        hasher.update(&self.0);
        let generic_array = hasher.finalize();
        builder.generic_byte_array(&generic_array).finalize()
    }

    fn tag() -> String {
        "TEST_TXN".to_string()
    }
}

impl Transaction for TestTransaction {}

/// A [`BlockPayload`] that contains a list of `TestTransaction`.
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct TestBlockPayload {
    /// List of transactions.
    pub transactions: Vec<TestTransaction>,
}

impl TestBlockPayload {
    /// Create a genesis block payload with bytes `vec![0]`, to be used for
    /// consensus task initiation.
    /// # Panics
    /// If the `VidScheme` construction fails.
    #[must_use]
    pub fn genesis() -> Self {
        TestBlockPayload {
            transactions: vec![],
        }
    }
}

impl Display for TestBlockPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlockPayload #txns={}", self.transactions.len())
    }
}

impl TestableBlock for TestBlockPayload {
    fn genesis() -> Self {
        Self::genesis()
    }

    fn txn_count(&self) -> u64 {
        self.transactions.len() as u64
    }
}

impl BlockPayload for TestBlockPayload {
    type Error = BlockError;
    type Transaction = TestTransaction;
    type Metadata = ();
    type Encode<'a> = <Vec<u8> as IntoIterator>::IntoIter;

    fn from_transactions(
        transactions: impl IntoIterator<Item = Self::Transaction>,
    ) -> Result<(Self, Self::Metadata), Self::Error> {
        let txns_vec: Vec<TestTransaction> = transactions.into_iter().collect();
        Ok((
            Self {
                transactions: txns_vec,
            },
            (),
        ))
    }

    fn from_bytes<E>(encoded_transactions: E, _metadata: &Self::Metadata) -> Self
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
            transactions.push(TestTransaction(
                encoded_vec[txn_start_index..next_index].to_vec(),
            ));
            current_index = next_index;
        }

        Self { transactions }
    }

    fn genesis() -> (Self, Self::Metadata) {
        (Self::genesis(), ())
    }

    fn encode(&self) -> Result<Self::Encode<'_>, Self::Error> {
        Ok(TestTransaction::encode(self.transactions.clone())?.into_iter())
    }

    fn transaction_commitments(
        &self,
        _metadata: &Self::Metadata,
    ) -> Vec<Commitment<Self::Transaction>> {
        self.transactions
            .iter()
            .map(commit::Committable::commit)
            .collect()
    }

    fn builder_commitment(&self, _metadata: &Self::Metadata) -> BuilderCommitment {
        let mut digest = sha2::Sha256::new();
        for txn in &self.transactions {
            digest.update(&txn.0);
        }
        BuilderCommitment::from_raw_digest(digest.finalize())
    }
}

/// A [`BlockHeader`] that commits to [`TestBlockPayload`].
#[derive(PartialEq, Eq, Hash, Clone, Debug, Deserialize, Serialize)]
pub struct TestBlockHeader {
    /// Block number.
    pub block_number: u64,
    /// VID commitment to the payload.
    pub payload_commitment: VidCommitment,
}

impl BlockHeader for TestBlockHeader {
    type Payload = TestBlockPayload;
    type State = TestValidatedState;

    fn new(
        _parent_state: &Self::State,
        _instance_state: &<Self::State as ValidatedState>::Instance,
        parent_header: &Self,
        payload_commitment: VidCommitment,
        _metadata: <Self::Payload as BlockPayload>::Metadata,
    ) -> Self {
        Self {
            block_number: parent_header.block_number + 1,
            payload_commitment,
        }
    }

    fn genesis(
        _instance_state: &<Self::State as ValidatedState>::Instance,
        payload_commitment: VidCommitment,
        _metadata: <Self::Payload as BlockPayload>::Metadata,
    ) -> Self {
        Self {
            block_number: 0,
            payload_commitment,
        }
    }

    fn block_number(&self) -> u64 {
        self.block_number
    }

    fn payload_commitment(&self) -> VidCommitment {
        self.payload_commitment
    }

    fn metadata(&self) -> &<Self::Payload as BlockPayload>::Metadata {
        &()
    }
}

impl Committable for TestBlockHeader {
    fn commit(&self) -> Commitment<Self> {
        RawCommitmentBuilder::new("Header Comm")
            .u64_field("block number", self.block_number())
            .constant_str("payload commitment")
            .fixed_size_bytes(self.payload_commitment().as_ref().as_ref())
            .finalize()
    }

    fn tag() -> String {
        "TEST_HEADER".to_string()
    }
}
