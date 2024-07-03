use std::marker::PhantomData;

use hotshot_types::{
    traits::{node_implementation::NodeType, signature_key::BuilderSignatureKey, BlockPayload},
    utils::BuilderCommitment,
    vid::{VidCommitment, VidPrecomputeData},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(bound = "")]
pub struct AvailableBlockInfo<TYPES: NodeType> {
    pub block_hash: BuilderCommitment,
    pub block_size: u64,
    pub offered_fee: u64,
    pub signature:
        <<TYPES as NodeType>::BuilderSignatureKey as BuilderSignatureKey>::BuilderSignature,
    pub sender: <TYPES as NodeType>::BuilderSignatureKey,
    pub _phantom: PhantomData<TYPES>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(bound = "")]
pub struct AvailableBlockData<TYPES: NodeType> {
    pub block_payload: TYPES::BlockPayload,
    pub metadata: <TYPES::BlockPayload as BlockPayload<TYPES>>::Metadata,
    pub signature:
        <<TYPES as NodeType>::BuilderSignatureKey as BuilderSignatureKey>::BuilderSignature,
    pub sender: <TYPES as NodeType>::BuilderSignatureKey,
}

impl<TYPES: NodeType> AvailableBlockData<TYPES> {
    pub fn validate_signature(&self) -> bool {
        // verify the signature over the message, construct the builder commitment
        let builder_commitment = self.block_payload.builder_commitment(&self.metadata);
        self.sender
            .validate_builder_signature(&self.signature, builder_commitment.as_ref())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(bound = "")]
pub struct AvailableBlockHeaderInput<TYPES: NodeType> {
    pub vid_commitment: VidCommitment,
    pub vid_precompute_data: VidPrecomputeData,
    // signature over vid_commitment, BlockPayload::Metadata, and offered_fee
    pub fee_signature:
        <<TYPES as NodeType>::BuilderSignatureKey as BuilderSignatureKey>::BuilderSignature,
    // signature over the current response
    pub message_signature:
        <<TYPES as NodeType>::BuilderSignatureKey as BuilderSignatureKey>::BuilderSignature,
    pub sender: <TYPES as NodeType>::BuilderSignatureKey,
}

impl<TYPES: NodeType> AvailableBlockHeaderInput<TYPES> {
    pub fn validate_signature(
        &self,
        offered_fee: u64,
        metadata: &<TYPES::BlockPayload as BlockPayload<TYPES>>::Metadata,
    ) -> bool {
        self.sender
            .validate_builder_signature(&self.message_signature, self.vid_commitment.as_ref())
            && self.sender.validate_fee_signature(
                &self.fee_signature,
                offered_fee,
                metadata,
                &self.vid_commitment,
            )
    }
}
