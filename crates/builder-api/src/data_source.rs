use async_trait::async_trait;
use hotshot_types::{
    traits::{node_implementation::NodeType, signature_key::SignatureKey},
    utils::BuilderCommitment,
    vid::VidCommitment,
};
use tagged_base64::TaggedBase64;

use crate::{
    block_info::{AvailableBlockData, AvailableBlockHeaderInput, AvailableBlockInfo},
    builder::BuildError,
};

#[async_trait]
pub trait BuilderDataSource<I>
where
    I: NodeType,
    <<I as NodeType>::SignatureKey as SignatureKey>::PureAssembledSignatureType:
        for<'a> TryFrom<&'a TaggedBase64> + Into<TaggedBase64>,
{
    async fn get_available_blocks(
        &self,
        for_parent: &VidCommitment,
    ) -> Result<Vec<AvailableBlockInfo<I>>, BuildError>;
    async fn claim_block(
        &self,
        block_hash: &BuilderCommitment,
        signature: &<<I as NodeType>::SignatureKey as SignatureKey>::PureAssembledSignatureType,
    ) -> Result<AvailableBlockData<I>, BuildError>;
    async fn claim_block_header_input(
        &self,
        block_hash: &BuilderCommitment,
        signature: &<<I as NodeType>::SignatureKey as SignatureKey>::PureAssembledSignatureType,
    ) -> Result<AvailableBlockHeaderInput<I>, BuildError>;
}

#[async_trait]
pub trait AcceptsTxnSubmits<I>
where
    I: NodeType,
{
    async fn submit_txn(&mut self, txn: <I as NodeType>::Transaction) -> Result<(), BuildError>;
}
