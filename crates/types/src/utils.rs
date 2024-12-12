// Copyright (c) 2021-2024 Espresso Systems (espressosys.com)
// This file is part of the HotShot repository.

// You should have received a copy of the MIT License
// along with the HotShot repository. If not, see <https://mit-license.org/>.

//! Utility functions, type aliases, helper structs and enum definitions.

use std::{
    hash::{Hash, Hasher},
    ops::Deref,
    sync::Arc,
};

use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use bincode::{
    config::{
        FixintEncoding, LittleEndian, RejectTrailing, WithOtherEndian, WithOtherIntEncoding,
        WithOtherLimit, WithOtherTrailing,
    },
    DefaultOptions, Options,
};
use committable::Commitment;
use digest::OutputSizeUser;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tagged_base64::tagged;
use typenum::Unsigned;

use crate::{
    data::Leaf2,
    traits::{node_implementation::NodeType, ValidatedState},
    vid::VidCommitment,
};

/// A view's state
#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(bound = "")]
pub enum ViewInner<TYPES: NodeType> {
    /// A pending view with an available block but not leaf proposal yet.
    ///
    /// Storing this state allows us to garbage collect blocks for views where a proposal is never
    /// made. This saves memory when a leader fails and subverts a DoS attack where malicious
    /// leaders repeatedly request availability for blocks that they never propose.
    Da {
        /// Payload commitment to the available block.
        payload_commitment: VidCommitment,
        /// An epoch to which the data belongs to. Relevant for validating against the correct stake table
        epoch: TYPES::Epoch,
    },
    /// Undecided view
    Leaf {
        /// Proposed leaf
        leaf: LeafCommitment<TYPES>,
        /// Validated state.
        state: Arc<TYPES::ValidatedState>,
        /// Optional state delta.
        delta: Option<Arc<<TYPES::ValidatedState as ValidatedState<TYPES>>::Delta>>,
        /// An epoch to which the data belongs to. Relevant for validating against the correct stake table
        epoch: TYPES::Epoch,
    },
    /// Leaf has failed
    Failed,
}
impl<TYPES: NodeType> Clone for ViewInner<TYPES> {
    fn clone(&self) -> Self {
        match self {
            Self::Da {
                payload_commitment,
                epoch,
            } => Self::Da {
                payload_commitment: *payload_commitment,
                epoch: *epoch,
            },
            Self::Leaf {
                leaf,
                state,
                delta,
                epoch,
            } => Self::Leaf {
                leaf: *leaf,
                state: Arc::clone(state),
                delta: delta.clone(),
                epoch: *epoch,
            },
            Self::Failed => Self::Failed,
        }
    }
}
/// The hash of a leaf.
pub type LeafCommitment<TYPES> = Commitment<Leaf2<TYPES>>;

/// Optional validated state and state delta.
pub type StateAndDelta<TYPES> = (
    Option<Arc<<TYPES as NodeType>::ValidatedState>>,
    Option<Arc<<<TYPES as NodeType>::ValidatedState as ValidatedState<TYPES>>::Delta>>,
);

impl<TYPES: NodeType> ViewInner<TYPES> {
    /// Return the underlying undecide leaf commitment and validated state if they exist.
    #[must_use]
    pub fn leaf_and_state(&self) -> Option<(LeafCommitment<TYPES>, &Arc<TYPES::ValidatedState>)> {
        if let Self::Leaf { leaf, state, .. } = self {
            Some((*leaf, state))
        } else {
            None
        }
    }

    /// return the underlying leaf hash if it exists
    #[must_use]
    pub fn leaf_commitment(&self) -> Option<LeafCommitment<TYPES>> {
        if let Self::Leaf { leaf, .. } = self {
            Some(*leaf)
        } else {
            None
        }
    }

    /// return the underlying validated state if it exists
    #[must_use]
    pub fn state(&self) -> Option<&Arc<TYPES::ValidatedState>> {
        if let Self::Leaf { state, .. } = self {
            Some(state)
        } else {
            None
        }
    }

    /// Return the underlying validated state and state delta if they exist.
    #[must_use]
    pub fn state_and_delta(&self) -> StateAndDelta<TYPES> {
        if let Self::Leaf { state, delta, .. } = self {
            (Some(Arc::clone(state)), delta.clone())
        } else {
            (None, None)
        }
    }

    /// return the underlying block paylod commitment if it exists
    #[must_use]
    pub fn payload_commitment(&self) -> Option<VidCommitment> {
        if let Self::Da {
            payload_commitment, ..
        } = self
        {
            Some(*payload_commitment)
        } else {
            None
        }
    }

    /// Returns `Epoch` if possible
    pub fn epoch(&self) -> Option<TYPES::Epoch> {
        match self {
            Self::Da { epoch, .. } | Self::Leaf { epoch, .. } => Some(*epoch),
            Self::Failed => None,
        }
    }
}

impl<TYPES: NodeType> Deref for View<TYPES> {
    type Target = ViewInner<TYPES>;

    fn deref(&self) -> &Self::Target {
        &self.view_inner
    }
}

/// This exists so we can perform state transitions mutably
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(bound = "")]
pub struct View<TYPES: NodeType> {
    /// The view data. Wrapped in a struct so we can mutate
    pub view_inner: ViewInner<TYPES>,
}

/// A struct containing information about a finished round.
#[derive(Debug, Clone)]
pub struct RoundFinishedEvent<TYPES: NodeType> {
    /// The round that finished
    pub view_number: TYPES::View,
}

/// Whether or not to stop inclusively or exclusively when walking
#[derive(Copy, Clone, Debug)]
pub enum Terminator<T> {
    /// Stop right before this view number
    Exclusive(T),
    /// Stop including this view number
    Inclusive(T),
}

/// Type alias for byte array of SHA256 digest length
type Sha256Digest = [u8; <sha2::Sha256 as OutputSizeUser>::OutputSize::USIZE];

#[tagged("BUILDER_COMMITMENT")]
#[derive(Clone, Debug, Hash, PartialEq, Eq, CanonicalSerialize, CanonicalDeserialize)]
/// Commitment that builders use to sign block options.
/// A thin wrapper around a Sha256 digest.
pub struct BuilderCommitment(Sha256Digest);

impl BuilderCommitment {
    /// Create new commitment for `data`
    pub fn from_bytes(data: impl AsRef<[u8]>) -> Self {
        Self(sha2::Sha256::digest(data.as_ref()).into())
    }

    /// Create a new commitment from a raw Sha256 digest
    pub fn from_raw_digest(digest: impl Into<Sha256Digest>) -> Self {
        Self(digest.into())
    }
}

impl AsRef<Sha256Digest> for BuilderCommitment {
    fn as_ref(&self) -> &Sha256Digest {
        &self.0
    }
}

/// For the wire format, we use bincode with the following options:
///   - No upper size limit
///   - Little endian encoding
///   - Varint encoding
///   - Reject trailing bytes
#[allow(clippy::type_complexity)]
#[must_use]
#[allow(clippy::type_complexity)]
pub fn bincode_opts() -> WithOtherTrailing<
    WithOtherIntEncoding<
        WithOtherEndian<WithOtherLimit<DefaultOptions, bincode::config::Infinite>, LittleEndian>,
        FixintEncoding,
    >,
    RejectTrailing,
> {
    bincode::DefaultOptions::new()
        .with_no_limit()
        .with_little_endian()
        .with_fixint_encoding()
        .reject_trailing_bytes()
}

/// Returns an epoch number given a block number and an epoch height
#[must_use]
pub fn epoch_from_block_number(block_number: u64, epoch_height: u64) -> u64 {
    if epoch_height == 0 {
        0
    } else if block_number % epoch_height == 0 {
        block_number / epoch_height
    } else {
        block_number / epoch_height + 1
    }
}

/// A function for generating a cute little user mnemonic from a hash
#[must_use]
pub fn mnemonic<H: Hash>(bytes: H) -> String {
    let mut state = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut state);
    mnemonic::to_string(state.finish().to_le_bytes())
}

/// A helper enum to indicate whether a node is in the epoch transition
/// A node is in epoch transition when its high QC is for the last block in an epoch
pub enum EpochTransitionIndicator {
    /// A node is currently in the epoch transition
    InTransition,
    /// A node is not in the epoch transition
    NotInTransition,
}
