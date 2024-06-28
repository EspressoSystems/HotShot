use hotshot_types::traits::{
    node_implementation::NodeType, signature_key::BuilderSignatureKey, BlockPayload,
};
use serde::{Deserialize, Serialize};

/// No changes to these types
pub use crate::v0_1::block_info::AvailableBlockInfo;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(bound = "")]
pub struct AvailableBlockData<TYPES: NodeType> {
    pub block_payload: TYPES::BlockPayload,
    pub metadata: <TYPES::BlockPayload as BlockPayload<TYPES>>::Metadata,
    pub fee: u64,
    pub signature:
        <<TYPES as NodeType>::BuilderSignatureKey as BuilderSignatureKey>::BuilderSignature,
    pub sender: <TYPES as NodeType>::BuilderSignatureKey,
}
