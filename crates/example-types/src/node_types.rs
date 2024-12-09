// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::marker::PhantomData;

pub use hotshot::traits::election::helpers::{
    RandomOverlapQuorumFilterConfig, StableQuorumFilterConfig,
};
use hotshot::traits::{
    election::{
        helpers::QuorumFilterConfig, randomized_committee::RandomizedCommittee,
        randomized_committee_members::RandomizedCommitteeMembers,
        static_committee::StaticCommittee,
        static_committee_leader_two_views::StaticCommitteeLeaderForTwoViews,
        two_static_committees::TwoStaticCommittees,
    },
    implementations::{CombinedNetworks, Libp2pNetwork, MemoryNetwork, PushCdnNetwork},
    NodeImplementation,
};
use hotshot_types::{
    data::{EpochNumber, ViewNumber},
    signature_key::{BLSPubKey, BuilderKey},
    traits::node_implementation::{NodeType, Versions},
};
use serde::{Deserialize, Serialize};
use vbs::version::StaticVersion;

use crate::{
    auction_results_provider_types::{TestAuctionResult, TestAuctionResultsProvider},
    block_types::{TestBlockHeader, TestBlockPayload, TestTransaction},
    state_types::{TestInstanceState, TestValidatedState},
    storage_types::TestStorage,
};

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
/// filler struct to implement node type and allow us
/// to select our traits
pub struct TestTypes;
impl NodeType for TestTypes {
    const EPOCH_HEIGHT: u64 = 10;

    type AuctionResult = TestAuctionResult;
    type View = ViewNumber;
    type Epoch = EpochNumber;
    type BlockHeader = TestBlockHeader;
    type BlockPayload = TestBlockPayload;
    type SignatureKey = BLSPubKey;
    type Transaction = TestTransaction;
    type ValidatedState = TestValidatedState;
    type InstanceState = TestInstanceState;
    type Membership = StaticCommittee<TestTypes>;
    type BuilderSignatureKey = BuilderKey;
}

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
/// filler struct to implement node type and allow us
/// to select our traits
pub struct TestTypesRandomizedLeader;
impl NodeType for TestTypesRandomizedLeader {
    const EPOCH_HEIGHT: u64 = 10;

    type AuctionResult = TestAuctionResult;
    type View = ViewNumber;
    type Epoch = EpochNumber;
    type BlockHeader = TestBlockHeader;
    type BlockPayload = TestBlockPayload;
    type SignatureKey = BLSPubKey;
    type Transaction = TestTransaction;
    type ValidatedState = TestValidatedState;
    type InstanceState = TestInstanceState;
    type Membership = RandomizedCommittee<TestTypesRandomizedLeader>;
    type BuilderSignatureKey = BuilderKey;
}

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
/// filler struct to implement node type and allow us
/// to select our traits
pub struct TestTypesRandomizedCommitteeMembers<CONFIG: QuorumFilterConfig> {
    _pd: PhantomData<CONFIG>,
}

impl<CONFIG: QuorumFilterConfig> NodeType for TestTypesRandomizedCommitteeMembers<CONFIG> {
    const EPOCH_HEIGHT: u64 = 10;

    type AuctionResult = TestAuctionResult;
    type View = ViewNumber;
    type Epoch = EpochNumber;
    type BlockHeader = TestBlockHeader;
    type BlockPayload = TestBlockPayload;
    type SignatureKey = BLSPubKey;
    type Transaction = TestTransaction;
    type ValidatedState = TestValidatedState;
    type InstanceState = TestInstanceState;
    type Membership =
        RandomizedCommitteeMembers<TestTypesRandomizedCommitteeMembers<CONFIG>, CONFIG>;
    type BuilderSignatureKey = BuilderKey;
}

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
/// filler struct to implement node type and allow us
/// to select our traits
pub struct TestConsecutiveLeaderTypes;
impl NodeType for TestConsecutiveLeaderTypes {
    const EPOCH_HEIGHT: u64 = 10;

    type AuctionResult = TestAuctionResult;
    type View = ViewNumber;
    type Epoch = EpochNumber;
    type BlockHeader = TestBlockHeader;
    type BlockPayload = TestBlockPayload;
    type SignatureKey = BLSPubKey;
    type Transaction = TestTransaction;
    type ValidatedState = TestValidatedState;
    type InstanceState = TestInstanceState;
    type Membership = StaticCommitteeLeaderForTwoViews<TestConsecutiveLeaderTypes>;
    type BuilderSignatureKey = BuilderKey;
}

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
/// filler struct to implement node type and allow us
/// to select our traits
pub struct TestTwoStakeTablesTypes;
impl NodeType for TestTwoStakeTablesTypes {
    const EPOCH_HEIGHT: u64 = 10;

    type AuctionResult = TestAuctionResult;
    type View = ViewNumber;
    type Epoch = EpochNumber;
    type BlockHeader = TestBlockHeader;
    type BlockPayload = TestBlockPayload;
    type SignatureKey = BLSPubKey;
    type Transaction = TestTransaction;
    type ValidatedState = TestValidatedState;
    type InstanceState = TestInstanceState;
    type Membership = TwoStaticCommittees<TestTwoStakeTablesTypes>;
    type BuilderSignatureKey = BuilderKey;
}

/// The Push CDN implementation
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct PushCdnImpl;

/// Memory network implementation
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct MemoryImpl;

/// Libp2p network implementation
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct Libp2pImpl;

/// Web server network implementation
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct WebImpl;

/// Combined Network implementation (libp2p + web server)
#[derive(Clone, Debug, Deserialize, Serialize, Hash, Eq, PartialEq)]
pub struct CombinedImpl;

/// static committee type alias
pub type StaticMembership = StaticCommittee<TestTypes>;

impl<TYPES: NodeType> NodeImplementation<TYPES> for PushCdnImpl {
    type Network = PushCdnNetwork<TYPES::SignatureKey>;
    type Storage = TestStorage<TYPES>;
    type AuctionResultsProvider = TestAuctionResultsProvider<TYPES>;
}

impl<TYPES: NodeType> NodeImplementation<TYPES> for MemoryImpl {
    type Network = MemoryNetwork<TYPES::SignatureKey>;
    type Storage = TestStorage<TYPES>;
    type AuctionResultsProvider = TestAuctionResultsProvider<TYPES>;
}

impl<TYPES: NodeType> NodeImplementation<TYPES> for CombinedImpl {
    type Network = CombinedNetworks<TYPES>;
    type Storage = TestStorage<TYPES>;
    type AuctionResultsProvider = TestAuctionResultsProvider<TYPES>;
}

impl<TYPES: NodeType> NodeImplementation<TYPES> for Libp2pImpl {
    type Network = Libp2pNetwork<TYPES>;
    type Storage = TestStorage<TYPES>;
    type AuctionResultsProvider = TestAuctionResultsProvider<TYPES>;
}

#[derive(Clone, Debug, Copy)]
pub struct TestVersions {}

impl Versions for TestVersions {
    type Base = StaticVersion<0, 1>;
    type Upgrade = StaticVersion<0, 2>;
    const UPGRADE_HASH: [u8; 32] = [
        1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
        0, 0,
    ];

    type Marketplace = StaticVersion<0, 3>;

    type Epochs = StaticVersion<0, 4>;
}

#[derive(Clone, Debug, Copy)]
pub struct MarketplaceUpgradeTestVersions {}

impl Versions for MarketplaceUpgradeTestVersions {
    type Base = StaticVersion<0, 2>;
    type Upgrade = StaticVersion<0, 3>;
    const UPGRADE_HASH: [u8; 32] = [
        1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
        0, 0,
    ];

    type Marketplace = StaticVersion<0, 3>;

    type Epochs = StaticVersion<0, 4>;
}

#[derive(Clone, Debug, Copy)]
pub struct MarketplaceTestVersions {}

impl Versions for MarketplaceTestVersions {
    type Base = StaticVersion<0, 3>;
    type Upgrade = StaticVersion<0, 3>;
    const UPGRADE_HASH: [u8; 32] = [
        1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
        0, 0,
    ];

    type Marketplace = StaticVersion<0, 3>;

    type Epochs = StaticVersion<0, 4>;
}

#[derive(Clone, Debug, Copy)]
pub struct EpochsTestVersions {}

impl Versions for EpochsTestVersions {
    type Base = StaticVersion<0, 4>;
    type Upgrade = StaticVersion<0, 4>;
    const UPGRADE_HASH: [u8; 32] = [
        1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
        0, 0,
    ];

    type Marketplace = StaticVersion<0, 3>;

    type Epochs = StaticVersion<0, 4>;
}

#[cfg(test)]
mod tests {
    use committable::{Commitment, Committable};
    use hotshot_types::{
        data::EpochNumber,
        impl_has_epoch,
        message::UpgradeLock,
        simple_vote::{HasEpoch, VersionedVoteData},
        traits::node_implementation::ConsensusTime,
    };
    use serde::{Deserialize, Serialize};

    use crate::node_types::{MarketplaceTestVersions, NodeType, TestTypes};
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Hash, Eq)]
    /// Dummy data used for test
    struct TestData<TYPES: NodeType> {
        data: u64,
        epoch: TYPES::Epoch,
    }

    impl<TYPES: NodeType> Committable for TestData<TYPES> {
        fn commit(&self) -> Commitment<Self> {
            committable::RawCommitmentBuilder::new("Test data")
                .u64(self.data)
                .finalize()
        }
    }

    impl_has_epoch!(TestData<TYPES>);

    #[tokio::test(flavor = "multi_thread")]
    /// Test that the view number affects the commitment post-marketplace
    async fn test_versioned_commitment_includes_view() {
        let upgrade_lock = UpgradeLock::new();

        let data = TestData {
            data: 10,
            epoch: EpochNumber::new(0),
        };

        let view_0 = <TestTypes as NodeType>::View::new(0);
        let view_1 = <TestTypes as NodeType>::View::new(1);

        let versioned_data_0 = VersionedVoteData::<
            TestTypes,
            TestData<TestTypes>,
            MarketplaceTestVersions,
        >::new(data, view_0, &upgrade_lock)
        .await
        .unwrap();
        let versioned_data_1 = VersionedVoteData::<
            TestTypes,
            TestData<TestTypes>,
            MarketplaceTestVersions,
        >::new(data, view_1, &upgrade_lock)
        .await
        .unwrap();

        let versioned_data_commitment_0: [u8; 32] = versioned_data_0.commit().into();
        let versioned_data_commitment_1: [u8; 32] = versioned_data_1.commit().into();

        assert!(
            versioned_data_commitment_0 != versioned_data_commitment_1,
            "left: {versioned_data_commitment_0:?}, right: {versioned_data_commitment_1:?}"
        );
    }
}
