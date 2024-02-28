use commit::Committable;
use hotshot::traits::implementations::MemoryStorage;
use hotshot::traits::Storage;
use hotshot_example_types::node_types::TestTypes;
use hotshot_types::{
    data::Leaf,
    simple_certificate::QuorumCertificate,
    traits::{
        node_implementation::{ConsensusTime, NodeType},
        signature_key::SignatureKey,
        storage::{StoredView, TestableStorage},
    },
};
use std::marker::PhantomData;
use tracing::instrument;

fn random_stored_view(view_number: <TestTypes as NodeType>::Time) -> StoredView<TestTypes> {
    let mut leaf = Leaf::genesis(&Default::default());
    leaf.view_number = view_number;
    let leaf_commit = leaf.commit();
    let data = hotshot_types::simple_vote::QuorumData { leaf_commit };
    let commit = data.commit();
    StoredView::from_qc_block_and_state(
        QuorumCertificate {
            is_genesis: view_number == <TestTypes as NodeType>::Time::genesis(),
            data,
            vote_commitment: commit,
            signatures: None,
            view_number,
            _pd: PhantomData,
        },
        leaf.block_header,
        leaf.block_payload,
        leaf_commit,
        <<TestTypes as NodeType>::SignatureKey as SignatureKey>::genesis_proposer_pk(),
    )
}

#[cfg(test)]
#[cfg_attr(
    async_executor_impl = "tokio",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(async_executor_impl = "async-std", async_std::test)]
#[instrument]
async fn memory_storage() {
    let storage = MemoryStorage::construct_tmp_storage().unwrap();
    let genesis = random_stored_view(<TestTypes as NodeType>::Time::genesis());
    storage
        .append_single_view(genesis.clone())
        .await
        .expect("Could not append block");
    assert_eq!(storage.get_anchored_view().await.unwrap(), genesis);
    storage
        .cleanup_storage_up_to_view(genesis.view_number)
        .await
        .unwrap();
    assert_eq!(storage.get_anchored_view().await.unwrap(), genesis);
    storage
        .cleanup_storage_up_to_view(genesis.view_number + 1)
        .await
        .unwrap();
    assert!(storage.get_anchored_view().await.is_err());
}
