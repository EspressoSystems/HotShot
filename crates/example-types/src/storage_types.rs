use async_lock::RwLock;
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};

use hotshot_types::{
    data::{DAProposal, VidDisperse},
    message::Proposal,
    traits::{
        block_storage::{BlockStorage, BlockStorageError, ProposalType},
        node_implementation::NodeType,
    },
    vote::HasViewNumber,
};

#[derive(Clone, Debug)]
pub struct TestBlockStorageInternal<TYPES: NodeType> {
    da_storage: HashMap<TYPES::Time, Proposal<TYPES, DAProposal<TYPES>>>,
    vid_storage: HashMap<TYPES::Time, Proposal<TYPES, VidDisperse<TYPES>>>,
}

impl<TYPES: NodeType> Default for TestBlockStorageInternal<TYPES> {
    fn default() -> Self {
        Self {
            da_storage: Default::default(),
            vid_storage: Default::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TestBlockStorage<TYPES: NodeType> {
    inner: Arc<RwLock<TestBlockStorageInternal<TYPES>>>,
}

impl<TYPES: NodeType> Default for TestBlockStorage<TYPES> {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(Default::default())),
        }
    }
}

#[async_trait]
impl<TYPES: NodeType> BlockStorage<TYPES> for TestBlockStorage<TYPES> {
    async fn append(&self, proposal: &ProposalType<TYPES>) -> Result<(), BlockStorageError> {
        match proposal {
            ProposalType::DAProposal(p) => {
                self.inner
                    .write()
                    .await
                    .da_storage
                    .insert(p.data.get_view_number(), p.clone());
            }
            ProposalType::VidDisperse(p) => {
                self.inner
                    .write()
                    .await
                    .vid_storage
                    .insert(p.data.get_view_number(), p.clone());
            }
        };

        Ok(())
    }
}
