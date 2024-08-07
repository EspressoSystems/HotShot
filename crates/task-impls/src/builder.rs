// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

use std::time::{Duration, Instant};

use async_compatibility_layer::art::async_sleep;
use hotshot_builder_api::v0_1::{
    block_info::AvailableBlockInfo,
    builder::{BuildError, Error as BuilderApiError},
};
use hotshot_types::{
    traits::{node_implementation::NodeType, signature_key::SignatureKey},
    vid::VidCommitment,
};
use serde::{Deserialize, Serialize};
use snafu::Snafu;
use surf_disco::{client::HealthStatus, Client, Url};
use tagged_base64::TaggedBase64;
use vbs::version::StaticVersionType;

#[derive(Debug, Snafu, Serialize, Deserialize)]
/// Represents errors than builder client may return
pub enum BuilderClientError {
    // NOTE: folds BuilderError::NotFound & builderError::Missing
    // into one. Maybe we'll want to handle that separately in
    // the future
    /// Block not found
    #[snafu(display("Requested block not found"))]
    NotFound,
    /// Generic error while accessing the API,
    /// i.e. when API isn't available or compatible
    #[snafu(display("Builder API error: {message}"))]
    Api {
        /// Underlying error
        message: String,
    },
}

impl From<BuilderApiError> for BuilderClientError {
    fn from(value: BuilderApiError) -> Self {
        match value {
            BuilderApiError::Request { source } | BuilderApiError::TxnUnpack { source } => {
                Self::Api {
                    message: source.to_string(),
                }
            }
            BuilderApiError::TxnSubmit { source } | BuilderApiError::BuilderAddress { source } => {
                Self::Api {
                    message: source.to_string(),
                }
            }
            BuilderApiError::Custom { message, .. } => Self::Api { message },
            BuilderApiError::BlockAvailable { source, .. }
            | BuilderApiError::BlockClaim { source, .. } => match source {
                BuildError::NotFound | BuildError::Missing => Self::NotFound,
                BuildError::Error { message } => Self::Api { message },
            },
        }
    }
}

/// Client for builder API
pub struct BuilderClient<TYPES: NodeType, Ver: StaticVersionType> {
    /// Underlying surf_disco::Client
    inner: Client<BuilderApiError, Ver>,
    /// Marker for [`NodeType`] used here
    _marker: std::marker::PhantomData<TYPES>,
}

impl<TYPES: NodeType, Ver: StaticVersionType> BuilderClient<TYPES, Ver> {
    /// Construct a new client from base url
    ///
    /// # Panics
    ///
    /// If the URL is malformed.
    pub fn new(base_url: impl Into<Url>) -> Self {
        Self {
            inner: Client::builder(base_url.into().join("block_info").unwrap())
                .set_timeout(Some(Duration::from_secs(2)))
                .build(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Wait for server to become available
    /// Returns `false` if server doesn't respond
    /// with OK healthcheck before `timeout`
    pub async fn connect(&self, timeout: Duration) -> bool {
        let timeout = Instant::now() + timeout;
        let mut backoff = Duration::from_millis(50);
        while Instant::now() < timeout {
            if matches!(
                self.inner.healthcheck::<HealthStatus>().await,
                Ok(HealthStatus::Available)
            ) {
                return true;
            }
            async_sleep(backoff).await;
            backoff *= 2;
        }
        false
    }

    /// Query builder for available blocks
    ///
    /// # Errors
    /// - [`BuilderClientError::NotFound`] if blocks aren't available for this parent
    /// - [`BuilderClientError::Api`] if API isn't responding or responds incorrectly
    pub async fn available_blocks(
        &self,
        parent: VidCommitment,
        view_number: u64,
        sender: TYPES::SignatureKey,
        signature: &<<TYPES as NodeType>::SignatureKey as SignatureKey>::PureAssembledSignatureType,
    ) -> Result<Vec<AvailableBlockInfo<TYPES>>, BuilderClientError> {
        let encoded_signature: TaggedBase64 = signature.clone().into();
        self.inner
            .get(&format!(
                "availableblocks/{parent}/{view_number}/{sender}/{encoded_signature}"
            ))
            .send()
            .await
            .map_err(Into::into)
    }
}

/// Version 0.1
pub mod v0_1 {

    use hotshot_builder_api::v0_1::block_info::{AvailableBlockData, AvailableBlockHeaderInput};
    pub use hotshot_builder_api::v0_1::Version;
    use hotshot_types::{
        traits::{node_implementation::NodeType, signature_key::SignatureKey},
        utils::BuilderCommitment,
    };
    use tagged_base64::TaggedBase64;

    use super::BuilderClientError;

    /// Client for builder API
    pub type BuilderClient<TYPES> = super::BuilderClient<TYPES, Version>;

    impl<TYPES: NodeType> BuilderClient<TYPES> {
        /// Claim block header input
        ///
        /// # Errors
        /// - [`BuilderClientError::NotFound`] if block isn't available
        /// - [`BuilderClientError::Api`] if API isn't responding or responds incorrectly
        pub async fn claim_block_header_input(
            &self,
            block_hash: BuilderCommitment,
            view_number: u64,
            sender: TYPES::SignatureKey,
            signature: &<<TYPES as NodeType>::SignatureKey as SignatureKey>::PureAssembledSignatureType,
        ) -> Result<AvailableBlockHeaderInput<TYPES>, BuilderClientError> {
            let encoded_signature: TaggedBase64 = signature.clone().into();
            self.inner
                .get(&format!(
                    "claimheaderinput/{block_hash}/{view_number}/{sender}/{encoded_signature}"
                ))
                .send()
                .await
                .map_err(Into::into)
        }

        /// Claim block
        ///
        /// # Errors
        /// - [`BuilderClientError::NotFound`] if block isn't available
        /// - [`BuilderClientError::Api`] if API isn't responding or responds incorrectly
        pub async fn claim_block(
            &self,
            block_hash: BuilderCommitment,
            view_number: u64,
            sender: TYPES::SignatureKey,
            signature: &<<TYPES as NodeType>::SignatureKey as SignatureKey>::PureAssembledSignatureType,
        ) -> Result<AvailableBlockData<TYPES>, BuilderClientError> {
            let encoded_signature: TaggedBase64 = signature.clone().into();
            self.inner
                .get(&format!(
                    "claimblock/{block_hash}/{view_number}/{sender}/{encoded_signature}"
                ))
                .send()
                .await
                .map_err(Into::into)
        }
    }
}

/// Version 0.2. No changes in API
pub mod v0_2 {
    use vbs::version::StaticVersion;

    pub use super::v0_1::*;

    /// Builder API version
    pub type Version = StaticVersion<0, 2>;
}

/// Version 0.3: marketplace. Bundles.
pub mod v0_3 {
    pub use hotshot_builder_api::v0_3::Version;
    use hotshot_types::{bundle::Bundle, traits::node_implementation::NodeType};
    use vbs::version::StaticVersion;

    pub use super::BuilderClientError;

    /// Client for builder API
    pub type BuilderClient<TYPES> = super::BuilderClient<TYPES, StaticVersion<0, 3>>;

    impl<TYPES: NodeType> BuilderClient<TYPES> {
        /// Claim block
        ///
        /// # Errors
        /// - [`BuilderClientError::NotFound`] if block isn't available
        /// - [`BuilderClientError::Api`] if API isn't responding or responds incorrectly
        pub async fn bundle(&self, view_number: u64) -> Result<Bundle<TYPES>, BuilderClientError> {
            self.inner
                .get(&format!("bundle/{view_number}"))
                .send()
                .await
                .map_err(Into::into)
        }
    }
}
