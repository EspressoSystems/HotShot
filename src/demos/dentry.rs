//! Double entry accounting demo
//!
//! This module provides an implementation of the `HotShot` suite of traits that implements a
//! basic demonstration of double entry accounting.
//!
//! These implementations are useful in examples and integration testing, but are not suitable for
//! production use.

use crate::traits::{
    election::static_committee::{StaticElectionConfig, StaticVoteToken},
    implementations::MemoryStorage,
    Block, NodeImplementation,
};
use commit::{Commitment, Committable};
use derivative::Derivative;
use espresso_systems_common::hotshot::tag;
use hotshot_types::{
    certificate::QuorumCertificate,
    constants::genesis_proposer_id,
    data::{random_commitment, LeafType, ValidatingLeaf, ValidatingProposal, ViewNumber},
    traits::{
        block_contents::Transaction,
        election::Election,
        network::CommunicationChannel,
        node_implementation::{ApplicationMetadata, NodeType},
        signature_key::ed25519::Ed25519Pub,
        state::{ConsensusTime, TestableBlock, TestableState, ValidatingConsensus},
        State,
    },
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use snafu::{ensure, Snafu};
use std::{
    collections::{BTreeMap, HashSet},
    fmt::Debug,
    marker::PhantomData,
};
use tracing::error;

/// application metadata stub
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct DEntryMetaData {}

impl ApplicationMetadata for DEntryMetaData {}

/// The account identifier type used by the demo
///
/// This is a type alias to [`String`] for simplicity.
pub type Account = String;

/// The account balance type used by the demo
///
/// This is a type alias to [`u64`] for simplicity.
pub type Balance = u64;

/// Records a reduction in an account balance
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct Subtraction {
    /// An account identifier
    pub account: Account,
    /// An account balance
    pub amount: Balance,
}

/// Records an increase in an account balance
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct Addition {
    /// An account identifier
    pub account: Account,
    /// An account balance
    pub amount: Balance,
}

/// The error type for the dentry demo
#[derive(Snafu, Debug)]
pub enum DEntryError {
    /// The subtraction and addition amounts for this transaction were not equal
    InconsistentTransaction,
    /// No such input account exists
    NoSuchInputAccount,
    /// No such output account exists
    NoSuchOutputAccount,
    /// Tried to move more money than was in the account
    InsufficentBalance,
    /// Previous state commitment does not match
    PreviousStateMismatch,
    /// Nonce was reused
    ReusedNonce,
    /// Genesis failure
    GenesisFailed,
    /// Genesis reencountered after initialization
    GenesisAfterStart,
}

/// The transaction for the dentry demo
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct DEntryTransaction {
    /// An increment to an account balance
    pub add: Addition,
    /// A decrement to an account balance
    pub sub: Subtraction,
    /// The nonce for a transaction, no two transactions can have the same nonce
    pub nonce: u64,
    /// Number of bytes to pad to each transaction
    pub padding: Vec<u8>,
}

impl Transaction for DEntryTransaction {}

impl DEntryTransaction {
    /// Ensures that this transaction is at least consistent with itself
    pub fn validate_independence(&self) -> bool {
        // Ensure that we are adding to one account exactly as much as we are subtracting from
        // another
        self.add.amount <= self.sub.amount // TODO why not strict equality?
    }
}

/// The state for the dentry demo
/// NOTE both fields are btrees because we need
/// ordered-ing otherwise commitments will not match
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct DEntryState {
    /// Key/value store of accounts and balances
    pub balances: BTreeMap<Account, Balance>,
    // /// Set of previously seen nonces
    // pub nonces: BTreeSet<u64>,
}

impl Committable for DEntryState {
    fn commit(&self) -> Commitment<Self> {
        let mut builder = commit::RawCommitmentBuilder::new("DEntry State Comm");

        for (k, v) in &self.balances {
            builder = builder.u64_field(k, *v);
        }
        builder = builder.constant_str("nonces");

        // for nonce in &self.nonces {
        //     builder = builder.u64(*nonce);
        // }

        builder.finalize()
    }

    fn tag() -> String {
        tag::DENTRY_STATE.to_string()
    }
}

/// initializes the first state on genesis
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct DEntryGenesisBlock {
    /// initializes the first state
    pub accounts: BTreeMap<Account, Balance>,
}

/// Any block after genesis
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub struct DEntryNormalBlock {
    /// Block state commitment
    pub previous_state: Commitment<DEntryState>,
    /// Transaction vector
    pub transactions: Vec<DEntryTransaction>,
}

/// The block for the dentry demo
#[derive(PartialEq, Eq, Hash, Serialize, Deserialize, Clone, Debug)]
pub enum DEntryBlock {
    /// genesis block
    Genesis(DEntryGenesisBlock),
    /// normal block
    Normal(DEntryNormalBlock),
}

impl Committable for DEntryBlock {
    fn commit(&self) -> Commitment<Self> {
        match &self {
            DEntryBlock::Genesis(block) => {
                let mut builder = commit::RawCommitmentBuilder::new("DEntry Genesis Comm")
                    .u64_field("account_count", block.accounts.len() as u64);
                for account in &block.accounts {
                    builder = builder.u64_field(account.0, *account.1);
                }
                builder.finalize()
            }
            DEntryBlock::Normal(block) => {
                let mut builder = commit::RawCommitmentBuilder::new("DEntry Block Comm")
                    .var_size_field("Previous State", block.previous_state.as_ref());

                for txn in &block.transactions {
                    builder = builder
                        .u64_field(&txn.add.account, txn.add.amount)
                        .u64_field(&txn.sub.account, txn.sub.amount)
                        .constant_str("nonce")
                        .u64_field("nonce", txn.nonce);
                }

                builder.finalize()
            }
        }
    }

    fn tag() -> String {
        tag::DENTRY_BLOCK.to_string()
    }
}

impl Committable for DEntryTransaction {
    fn commit(&self) -> Commitment<Self> {
        commit::RawCommitmentBuilder::new("DEntry Txn Comm")
            .u64_field(&self.add.account, self.add.amount)
            .u64_field(&self.sub.account, self.sub.amount)
            .constant_str("nonce")
            .u64_field("nonce", self.nonce)
            .finalize()
    }

    fn tag() -> String {
        tag::DENTRY_TXN.to_string()
    }
}

impl DEntryBlock {
    /// generate a default genesis block
    pub fn genesis() -> Self {
        let accounts: BTreeMap<Account, Balance> = vec![
            ("Joe", 1_000_000),
            ("Nathan M", 500_000),
            ("John", 400_000),
            ("Nathan Y", 600_000),
            ("Ian", 5_000_000),
        ]
        .into_iter()
        .map(|(x, y)| (x.to_string(), y))
        .collect();
        Self::Genesis(DEntryGenesisBlock { accounts })
    }

    /// generate a genesis block with the provided initial accounts and balances
    pub fn genesis_from(accounts: BTreeMap<Account, Balance>) -> Self {
        Self::Genesis(DEntryGenesisBlock { accounts })
    }

    /// total transactions in this block
    pub fn txn_count(&self) -> u64 {
        if let DEntryBlock::Normal(block) = self {
            block.transactions.len() as u64
        } else {
            0
        }
    }
}

impl State for DEntryState {
    type Error = DEntryError;

    type BlockType = DEntryBlock;

    type Time = ViewNumber;

    type ConsensusType = ValidatingConsensus;

    fn next_block(&self) -> Self::BlockType {
        DEntryBlock::Normal(DEntryNormalBlock {
            previous_state: self.commit(),
            transactions: Vec::new(),
        })
    }

    // Note: validate_block is actually somewhat redundant, its meant to be a quick and dirty check
    // for clarity, the logic is duplicated with append_to
    fn validate_block(&self, block: &Self::BlockType, _view_number: &Self::Time) -> bool {
        match block {
            DEntryBlock::Genesis(_) => self.balances.is_empty(), //  && self.nonces.is_empty(),
            DEntryBlock::Normal(block) => {
                let state = self;
                // A valid block is one in which every transaction is internally consistent, and results in
                // nobody having a negative balance
                //
                // We will check this, in this case, by making a trial copy of our balances map, making
                // trial modifications to it, and then asserting that no balances are negative
                let mut trial_balances = state.balances.clone();
                for tx in &block.transactions {
                    // This is a macro from SNAFU that returns an Err if the condition is not satisfied
                    //
                    // We first check that the transaction is internally consistent, then apply the change
                    // to our trial map
                    if !tx.validate_independence() {
                        error!("validate_independence failed");
                        return false;
                    }
                    // Find the input account, and subtract the transfer balance from it, failing if it
                    // doesn't exist
                    if let Some(input_account) = trial_balances.get_mut(&tx.sub.account) {
                        *input_account -= tx.sub.amount;
                    } else {
                        error!("no such input account");
                        return false;
                    }
                    // Find the output account, and add the transfer balance to it, failing if it doesn't
                    // exist
                    if let Some(output_account) = trial_balances.get_mut(&tx.add.account) {
                        *output_account += tx.add.amount;
                    } else {
                        error!("no such output account");
                        return false;
                    }
                    // // Check to make sure the nonce isn't used
                    // if state.nonces.contains(&tx.nonce) {
                    //     warn!(?state, ?tx, "State nonce is used for transaction");
                    //     return false;
                    // }
                }
                // This block has now passed all our tests, and thus has not done anything bad, so the block
                // is valid if its previous state hash matches that of the previous state
                let result = block.previous_state == state.commit();
                if !result {
                    error!(
                        "hash failure. previous_block: {:?} hash_state: {:?}",
                        block.previous_state,
                        state.commit()
                    );
                }
                result
            }
        }
    }

    fn append(
        &self,
        block: &Self::BlockType,
        _view_number: &Self::Time,
    ) -> std::result::Result<Self, Self::Error> {
        match block {
            DEntryBlock::Genesis(block) => {
                if self.balances.is_empty() {
                    // && self.nonces.is_empty()
                    let mut new_state = Self::default();
                    for account in &block.accounts {
                        if new_state
                            .balances
                            .insert(account.0.clone(), *account.1)
                            .is_some()
                        {
                            error!("Adding the same account twice during application of genesis block!");
                            return Err(DEntryError::GenesisFailed);
                        }
                    }
                    Ok(new_state)
                } else {
                    Err(DEntryError::GenesisAfterStart)
                }
            }
            DEntryBlock::Normal(block) => {
                let state = self;
                // A valid block is one in which every transaction is internally consistent, and results in
                // nobody having a negative balance
                //
                // We will check this, in this case, by making a trial copy of our balances map, making
                // trial modifications to it, and then asserting that no balances are negative
                let mut trial_balances = state.balances.clone();
                for tx in &block.transactions {
                    // This is a macro from SNAFU that returns an Err if the condition is not satisfied
                    //
                    // We first check that the transaction is internally consistent, then apply the change
                    // to our trial map
                    ensure!(tx.validate_independence(), InconsistentTransactionSnafu);
                    // Find the input account, and subtract the transfer balance from it, failing if it
                    // doesn't exist
                    if let Some(input_account) = trial_balances.get_mut(&tx.sub.account) {
                        *input_account -= tx.sub.amount;
                    } else {
                        return Err(DEntryError::NoSuchInputAccount);
                    }
                    // Find the output account, and add the transfer balance to it, failing if it doesn't
                    // exist
                    if let Some(output_account) = trial_balances.get_mut(&tx.add.account) {
                        *output_account += tx.add.amount;
                    } else {
                        return Err(DEntryError::NoSuchOutputAccount);
                    }
                    // // Check for nonce reuse
                    // if state.nonces.contains(&tx.nonce) {
                    //     return Err(DEntryError::ReusedNonce);
                    // }
                }
                // Make sure our previous state commitment matches the provided state
                if block.previous_state == state.commit() {
                    // This block has now passed all our tests, and thus has not done anything bad
                    // Add the nonces from this block
                    // let mut nonces = state.nonces.clone();
                    // for tx in &block.transactions {
                    //     nonces.insert(tx.nonce);
                    // }
                    Ok(DEntryState {
                        balances: trial_balances,
                        // nonces,
                    })
                } else {
                    Err(DEntryError::PreviousStateMismatch)
                }
            }
        }
    }

    fn on_commit(&self) {
        // Does nothing in this implementation
    }
}

impl TestableState for DEntryState {
    fn create_random_transaction(
        &self,
        rng: &mut dyn rand::RngCore,
        padding: u64,
    ) -> <Self::BlockType as Block>::Transaction {
        use rand::seq::IteratorRandom;

        let non_zero_balances = self
            .balances
            .iter()
            .filter(|b| *b.1 > 0)
            .collect::<Vec<_>>();

        assert!(
            !non_zero_balances.is_empty(),
            "No nonzero balances were available! Unable to generate transaction."
        );

        let input_account = non_zero_balances.iter().choose(rng).unwrap().0;
        let output_account = self.balances.keys().choose(rng).unwrap();
        let amount = rng.gen_range(0..100);

        DEntryTransaction {
            add: Addition {
                account: output_account.to_string(),
                amount,
            },
            sub: Subtraction {
                account: input_account.to_string(),
                amount,
            },
            nonce: rng.gen(),
            padding: vec![0; padding as usize],
        }
    }
}

impl TestableBlock for DEntryBlock {
    fn genesis() -> Self {
        Self::genesis()
    }

    fn txn_count(&self) -> u64 {
        Self::txn_count(self)
    }
}

impl Block for DEntryBlock {
    type Transaction = DEntryTransaction;

    type Error = DEntryError;

    fn new() -> Self {
        <Self as TestableBlock>::genesis()
    }

    fn add_transaction_raw(
        &self,
        tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error> {
        match self {
            DEntryBlock::Genesis(_) => Err(DEntryError::GenesisAfterStart),
            DEntryBlock::Normal(block) => {
                // first, make sure that the transaction is internally valid
                if tx.validate_independence() {
                    // Then check the previous transactions in the block
                    if block.transactions.iter().any(|x| x.nonce == tx.nonce) {
                        return Err(DEntryError::ReusedNonce);
                    }
                    let mut new_block = block.clone();
                    // Insert our transaction and return
                    new_block.transactions.push(tx.clone());
                    Ok(DEntryBlock::Normal(new_block))
                } else {
                    Err(DEntryError::InconsistentTransaction)
                }
            }
        }
    }
    fn contained_transactions(&self) -> HashSet<Commitment<DEntryTransaction>> {
        match self {
            DEntryBlock::Genesis(_) => HashSet::new(),
            DEntryBlock::Normal(block) => block
                .transactions
                .clone()
                .into_iter()
                .map(|tx| tx.commit())
                .collect(),
        }
    }
}

/// Implementation of [`NodeType`] for [`DEntryNode`]
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
pub struct DEntryTypes;

impl NodeType for DEntryTypes {
    type ConsensusType = ValidatingConsensus;
    type Time = ViewNumber;
    type BlockType = DEntryBlock;
    type SignatureKey = Ed25519Pub;
    type VoteTokenType = StaticVoteToken<Self::SignatureKey>;
    type Transaction = DEntryTransaction;
    type ElectionConfigType = StaticElectionConfig;
    type StateType = DEntryState;
    type ApplicationMetadataType = DEntryMetaData;
}

/// The node implementation for the dentry demo
#[derive(Derivative)]
#[derivative(Clone(bound = ""))]
pub struct DEntryNode<NET, ELE>(PhantomData<NET>, PhantomData<ELE>)
where
    NET: CommunicationChannel<
        DEntryTypes,
        ValidatingLeaf<DEntryTypes>,
        ValidatingProposal<DEntryTypes, ELE>,
        ELE,
    >,
    ELE: Election<DEntryTypes, LeafType = ValidatingLeaf<DEntryTypes>>;

impl<NET, ELE> DEntryNode<NET, ELE>
where
    NET: CommunicationChannel<
        DEntryTypes,
        ValidatingLeaf<DEntryTypes>,
        ValidatingProposal<DEntryTypes, ELE>,
        ELE,
    >,
    ELE: Election<DEntryTypes, LeafType = ValidatingLeaf<DEntryTypes>>,
{
    /// Create a new `DEntryNode`
    pub fn new() -> Self {
        DEntryNode(PhantomData, PhantomData)
    }
}

impl<NET, ELE> Debug for DEntryNode<NET, ELE>
where
    NET: CommunicationChannel<
        DEntryTypes,
        ValidatingLeaf<DEntryTypes>,
        ValidatingProposal<DEntryTypes, ELE>,
        ELE,
    >,
    ELE: Election<DEntryTypes, LeafType = ValidatingLeaf<DEntryTypes>>,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DEntryNode")
            .field("_phantom", &"phantom")
            .finish()
    }
}

impl<NET, ELE> Default for DEntryNode<NET, ELE>
where
    NET: CommunicationChannel<
        DEntryTypes,
        ValidatingLeaf<DEntryTypes>,
        ValidatingProposal<DEntryTypes, ELE>,
        ELE,
    >,
    ELE: Election<DEntryTypes, LeafType = ValidatingLeaf<DEntryTypes>>,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<NET, ELE> NodeImplementation<DEntryTypes> for DEntryNode<NET, ELE>
where
    NET: CommunicationChannel<
        DEntryTypes,
        ValidatingLeaf<DEntryTypes>,
        ValidatingProposal<DEntryTypes, ELE>,
        ELE,
    >,
    ELE: Election<DEntryTypes, LeafType = ValidatingLeaf<DEntryTypes>> + Debug,
{
    type Leaf = ValidatingLeaf<DEntryTypes>;
    type Storage = MemoryStorage<DEntryTypes, ELE::LeafType>;
    type Networking = NET;
    type Election = ELE;
    type Proposal = ValidatingProposal<DEntryTypes, ELE>;
}

/// Provides a random [`QuorumCertificate`]
pub fn random_quorum_certificate<TYPES: NodeType, LEAF: LeafType<NodeType = TYPES>>(
    rng: &mut dyn rand::RngCore,
) -> QuorumCertificate<TYPES, LEAF> {
    QuorumCertificate {
        // block_commitment: random_commitment(rng),
        leaf_commitment: random_commitment(rng),
        view_number: TYPES::Time::new(rng.gen()),
        signatures: BTreeMap::default(),
        is_genesis: rng.gen(),
    }
}

/// Provides a random [`Leaf`]
pub fn random_validating_leaf<TYPES: NodeType<ConsensusType = ValidatingConsensus>>(
    deltas: TYPES::BlockType,
    rng: &mut dyn rand::RngCore,
) -> ValidatingLeaf<TYPES>
where
    TYPES::StateType: TestableState,
    TYPES::BlockType: TestableBlock,
{
    let justify_qc = random_quorum_certificate(rng);
    let state = TYPES::StateType::default()
        .append(&deltas, &TYPES::Time::new(42))
        .unwrap_or_default();
    ValidatingLeaf {
        view_number: justify_qc.view_number,
        height: rng.next_u64(),
        justify_qc,
        parent_commitment: random_commitment(rng),
        deltas,
        state,
        rejected: Vec::new(),
        timestamp: time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
        proposer_id: genesis_proposer_id(),
    }
}
