// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Vote, Accumulator, and Certificate Types

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Display,
    marker::PhantomData,
};

use anyhow::Result;
use bitvec::{bitvec, vec::BitVec};
use committable::{Commitment, Committable};
use either::Either;
use ethereum_types::U256;
use tracing::error;

use crate::{
    message::UpgradeLock,
    simple_certificate::Threshold,
    simple_vote::{VersionedVoteData, Voteable},
    traits::{
        election::Membership,
        node_implementation::{ConsensusTime, NodeType, Versions},
        signature_key::{SignatureKey, StakeTableEntryType},
    },
};

/// A simple vote that has a signer and commitment to the data voted on.
pub trait Vote<Key: SignatureKey, Time: ConsensusTime + Display>: HasViewNumber<Time> {
    /// Type of data commitment this vote uses.
    type Commitment: Voteable;

    /// Get the signature of the vote sender
    fn signature(&self) -> <Key as SignatureKey>::PureAssembledSignatureType;
    /// Gets the data which was voted on by this vote
    fn date(&self) -> &Self::Commitment;
    /// Gets the Data commitment of the vote
    fn date_commitment(&self) -> Commitment<Self::Commitment>;

    /// Gets the public signature key of the votes creator/sender
    fn signing_key(&self) -> Key;
}

/// Any type that is associated with a view
pub trait HasViewNumber<Time: ConsensusTime + Display> {
    /// Returns the view number the type refers to.
    fn view_number(&self) -> Time;
}

/**
The certificate formed from the collection of signatures a committee.
The committee is defined by the `Membership` associated type.
The votes all must be over the `Commitment` associated type.
*/
pub trait Certificate<Key: SignatureKey, Time: ConsensusTime + Display>:
    HasViewNumber<Time>
{
    /// The data commitment this certificate certifies.
    type Voteable: Voteable;

    /// Threshold Functions
    type Threshold: Threshold<Key, Time>;

    /// Build a certificate from the data commitment and the quorum of signers
    fn create_signed_certificate<V: Versions>(
        vote_commitment: Commitment<VersionedVoteData<TYPES, Self::Voteable, V>>,
        data: Self::Voteable,
        sig: <Key as SignatureKey>::QcType,
        view: Time,
    ) -> Self;

    /// Checks if the cert is valid
    fn is_valid_cert<MEMBERSHIP: Membership<Key, Time>, V: Versions>(
        &self,
        membership: &MEMBERSHIP,
        upgrade_lock: &UpgradeLock<Key, Time, V>,
    ) -> impl std::future::Future<Output = bool>;
    /// Returns the amount of stake needed to create this certificate
    // TODO: Make this a static ratio of the total stake of `Membership`
    fn threshold<MEMBERSHIP: Membership<Key, Time>>(membership: &MEMBERSHIP) -> u64;
    /// Get the commitment which was voted on
    fn date(&self) -> &Self::Voteable;
    /// Get the vote commitment which the votes commit to
    fn date_commitment<V: Versions>(
        &self,
        upgrade_lock: &UpgradeLock<Key, Time, V>,
    ) -> impl std::future::Future<Output = Result<Commitment<VersionedVoteData<Time, Self::Voteable, V>>>>;
}
/// Mapping of vote commitment to signatures and bitvec
type SignersMap<COMMITMENT, KEY> = HashMap<
    COMMITMENT,
    (
        BitVec,
        Vec<<KEY as SignatureKey>::PureAssembledSignatureType>,
    ),
>;

#[allow(clippy::type_complexity)]
/// Accumulates votes until a certificate is formed.  This implementation works for all simple vote and certificate pairs
pub struct VoteAccumulator<
    Key: SignatureKey,
    Time: ConsensusTime + Display,
    VOTE: Vote<Key, Time>,
    CERT: Certificate<Key, Time, Voteable = VOTE::Commitment>,
    V: Versions,
> {
    /// Map of all signatures accumulated so far
    pub vote_outcomes: VoteMap2<
        Commitment<VersionedVoteData<Key, Time, <VOTE as Vote<Key, Time>>::Commitment, V>>,
        Key,
        <Key as SignatureKey>::PureAssembledSignatureType,
    >,
    /// A bitvec to indicate which node is active and send out a valid signature for certificate aggregation, this automatically do uniqueness check
    /// And a list of valid signatures for certificate aggregation
    pub signers: SignersMap<
        Commitment<VersionedVoteData<Key, Time, <VOTE as Vote<Key, Time>>::Commitment, V>>,
        Key,
    >,
    /// Phantom data to specify the types this accumulator is for
    pub phantom: PhantomData<(Key, Time, VOTE, CERT)>,
    /// version information
    pub upgrade_lock: UpgradeLock<Key, Time, V>,
}

impl<
        Key: SignatureKey,
        Time: ConsensusTime + Display,
        Membership: Membership<Key, Time>,
        VOTE: Vote<Key, Time>,
        CERT: Certificate<Key, Time, Voteable = VOTE::Commitment>,
        V: Versions,
    > VoteAccumulator<Key, Time, VOTE, CERT, V>
{
    /// Add a vote to the total accumulated votes.  Returns the accumulator or the certificate if we
    /// have accumulated enough votes to exceed the threshold for creating a certificate.
    pub async fn accumulate(&mut self, vote: &VOTE, membership: &Membership) -> Either<(), CERT> {
        let key = vote.signing_key();

        let vote_commitment = match VersionedVoteData::new(
            vote.date().clone(),
            vote.view_number(),
            &self.upgrade_lock,
        )
        .await
        {
            Ok(data) => data.commit(),
            Err(e) => {
                tracing::warn!("Failed to generate versioned vote data: {e}");
                return Either::Left(());
            }
        };

        if !key.validate(&vote.signature(), vote_commitment.as_ref()) {
            error!("Invalid vote! Vote Data {:?}", vote.date());
            return Either::Left(());
        }

        let Some(stake_table_entry) = membership.stake(&key) else {
            return Either::Left(());
        };
        let stake_table = membership.stake_table();
        let Some(vote_node_id) = stake_table
            .iter()
            .position(|x| *x == stake_table_entry.clone())
        else {
            return Either::Left(());
        };

        let original_signature: <Key as SignatureKey>::PureAssembledSignatureType =
            vote.signature();

        let (total_stake_casted, total_vote_map) = self
            .vote_outcomes
            .entry(vote_commitment)
            .or_insert_with(|| (U256::from(0), BTreeMap::new()));

        // Check for duplicate vote
        if total_vote_map.contains_key(&key) {
            return Either::Left(());
        }
        let (signers, sig_list) = self
            .signers
            .entry(vote_commitment)
            .or_insert((bitvec![0; membership.total_nodes()], Vec::new()));
        if signers.get(vote_node_id).as_deref() == Some(&true) {
            error!("Node id is already in signers list");
            return Either::Left(());
        }
        signers.set(vote_node_id, true);
        sig_list.push(original_signature);

        // TODO: Get the stake from the stake table entry.
        *total_stake_casted += stake_table_entry.stake();
        total_vote_map.insert(key, (vote.signature(), vote_commitment));

        if *total_stake_casted >= CERT::threshold(membership).into() {
            // Assemble QC
            let real_qc_pp: <Key as SignatureKey>::QcParams =
                <Key as SignatureKey>::public_parameter(
                    stake_table,
                    U256::from(CERT::threshold(membership)),
                );

            let real_qc_sig =
                <Key as SignatureKey>::assemble(&real_qc_pp, signers.as_bitslice(), &sig_list[..]);

            let cert = CERT::create_signed_certificate::<V>(
                vote_commitment,
                vote.date().clone(),
                real_qc_sig,
                vote.view_number(),
            );
            return Either::Right(cert);
        }
        Either::Left(())
    }
}

/// Mapping of commitments to vote tokens by key.
type VoteMap2<COMMITMENT, PK, SIG> = HashMap<COMMITMENT, (U256, BTreeMap<PK, (SIG, COMMITMENT)>)>;
