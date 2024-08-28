// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use hotshot::traits::{
    election::{
        static_committee::{GeneralStaticCommittee, StaticCommittee},
        static_committee_leader_two_views::StaticCommitteeLeaderForTwoViews,
    },
    implementations::{CombinedNetworks, Libp2pNetwork, MemoryNetwork, PushCdnNetwork},
    NodeImplementation,
};
use hotshot_types::{
    data::ViewNumber,
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
    type AuctionResult = TestAuctionResult;
    type Time = ViewNumber;
    type BlockHeader = TestBlockHeader;
    type BlockPayload = TestBlockPayload;
    type SignatureKey = BLSPubKey;
    type Transaction = TestTransaction;
    type ValidatedState = TestValidatedState;
    type InstanceState = TestInstanceState;
    type Membership = GeneralStaticCommittee<TestTypes, Self::SignatureKey>;
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
    type AuctionResult = TestAuctionResult;
    type Time = ViewNumber;
    type BlockHeader = TestBlockHeader;
    type BlockPayload = TestBlockPayload;
    type SignatureKey = BLSPubKey;
    type Transaction = TestTransaction;
    type ValidatedState = TestValidatedState;
    type InstanceState = TestInstanceState;
    type Membership =
        StaticCommitteeLeaderForTwoViews<TestConsecutiveLeaderTypes, Self::SignatureKey>;
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

/// Combined Network implementation (libp2p + web sever)
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
    type Network = Libp2pNetwork<TYPES::SignatureKey>;
    type Storage = TestStorage<TYPES>;
    type AuctionResultsProvider = TestAuctionResultsProvider<TYPES>;
}

#[derive(Clone, Debug, Copy)]
pub struct TestVersions {}

impl Versions for TestVersions {
    type Base = StaticVersion<0, 3>;
    type Upgrade = StaticVersion<0, 3>;
    const UPGRADE_HASH: [u8; 32] = [
        1, 0, 1, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0,
        0, 0,
    ];

    type Marketplace = StaticVersion<0, 3>;
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
}

#[cfg(test)]
mod tests {
    use committable::{Commitment, Committable};
    use hotshot_types::{
        message::UpgradeLock, simple_vote::VersionedVoteData,
        traits::node_implementation::ConsensusTime,
    };
    use serde::{Deserialize, Serialize};

    use crate::node_types::{MarketplaceTestVersions, NodeType, TestTypes, TestVersions};
    #[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Hash, Eq)]
    /// Dummy data used for test
    struct TestData {
        data: u64,
    }

    impl Committable for TestData {
        fn commit(&self) -> Commitment<Self> {
            committable::RawCommitmentBuilder::new("Test data")
                .u64(self.data)
                .finalize()
        }
    }

    #[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn test_versioned_commitment() {
        let view = <TestTypes as NodeType>::Time::new(0);
        let upgrade_lock = UpgradeLock::new();

        let data = TestData { data: 10 };
        let data_commitment: [u8; 32] = data.commit().into();

        let versioned_data =
            VersionedVoteData::<TestTypes, TestData, TestVersions>::new(data, view, &upgrade_lock)
                .await
                .unwrap();
        let versioned_data_commitment: [u8; 32] = versioned_data.commit().into();

        assert_eq!(versioned_data_commitment, data_commitment);
    }

    #[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    /// Test that the view number affects the commitment post-marketplace
    async fn test_versioned_commitment_includes_view() {
        let upgrade_lock = UpgradeLock::new();

        let data = TestData { data: 10 };

        let view_0 = <TestTypes as NodeType>::Time::new(0);
        let view_1 = <TestTypes as NodeType>::Time::new(1);

        let versioned_data_0 =
            VersionedVoteData::<TestTypes, TestData, MarketplaceTestVersions>::new(
                data,
                view_0,
                &upgrade_lock,
            )
            .await
            .unwrap();
        let versioned_data_1 =
            VersionedVoteData::<TestTypes, TestData, MarketplaceTestVersions>::new(
                data,
                view_1,
                &upgrade_lock,
            )
            .await
            .unwrap();

        let versioned_data_commitment_0: [u8; 32] = versioned_data_0.commit().into();
        let versioned_data_commitment_1: [u8; 32] = versioned_data_1.commit().into();

        assert!(
            versioned_data_commitment_0 != versioned_data_commitment_1,
            "left: {versioned_data_commitment_0:?}, right: {versioned_data_commitment_1:?}"
        );
    }

    #[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    /// Test that the view number does not affect the commitment pre-marketplace
    async fn test_versioned_commitment_excludes_view() {
        let upgrade_lock = UpgradeLock::new();

        let data = TestData { data: 10 };

        let view_0 = <TestTypes as NodeType>::Time::new(0);
        let view_1 = <TestTypes as NodeType>::Time::new(1);

        let versioned_data_0 = VersionedVoteData::<TestTypes, TestData, TestVersions>::new(
            data,
            view_0,
            &upgrade_lock,
        )
        .await
        .unwrap();
        let versioned_data_1 = VersionedVoteData::<TestTypes, TestData, TestVersions>::new(
            data,
            view_1,
            &upgrade_lock,
        )
        .await
        .unwrap();

        let versioned_data_commitment_0: [u8; 32] = versioned_data_0.commit().into();
        let versioned_data_commitment_1: [u8; 32] = versioned_data_1.commit().into();

        assert!(
            versioned_data_commitment_0 == versioned_data_commitment_1,
            "left: {versioned_data_commitment_0:?}, right: {versioned_data_commitment_1:?}"
        );
    }
}
