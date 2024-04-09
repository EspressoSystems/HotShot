use std::{hash::Hash, marker::PhantomData};

use hotshot_types::{
    traits::{node_implementation::NodeType, signature_key::BuilderSignatureKey, BlockPayload},
    utils::BuilderCommitment,
    vid::{VidCommitment, VidPrecomputeData},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(bound = "")]
pub struct AvailableBlockInfo<I: NodeType> {
    pub block_hash: BuilderCommitment,
    pub block_size: u64,
    pub offered_fee: u64,
    pub signature: <<I as NodeType>::BuilderSignatureKey as BuilderSignatureKey>::BuilderSignature,
    pub sender: <I as NodeType>::BuilderSignatureKey,
    pub _phantom: PhantomData<I>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(bound = "")]
pub struct AvailableBlockData<I: NodeType> {
    pub block_payload: <I as NodeType>::BlockPayload,
    pub metadata: <<I as NodeType>::BlockPayload as BlockPayload>::Metadata,
    pub signature: <<I as NodeType>::BuilderSignatureKey as BuilderSignatureKey>::BuilderSignature,
    pub sender: <I as NodeType>::BuilderSignatureKey,
    pub _phantom: PhantomData<I>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(bound = "")]
pub struct AvailableBlockHeaderInput<I: NodeType> {
    pub vid_commitment: VidCommitment,
    pub vid_precompute_data: VidPrecomputeData,
    // signature over vid_commitment, BlockPayload::Metadata, and offered_fee
    pub fee_signature:
        <<I as NodeType>::BuilderSignatureKey as BuilderSignatureKey>::BuilderSignature,
    // signature over the current response
    pub message_signature:
        <<I as NodeType>::BuilderSignatureKey as BuilderSignatureKey>::BuilderSignature,
    pub sender: <I as NodeType>::BuilderSignatureKey,
    pub _phantom: PhantomData<I>,
}
