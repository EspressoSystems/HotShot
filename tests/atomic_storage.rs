mod common;

use phaselock::{
    data::{Leaf, QuorumCertificate, StateHash},
    demos::dentry::{
        random_leaf, random_quorom_certificate, random_transaction, DEntryBlock, State,
    },
    traits::{BlockContents, Storage, StorageResult},
    H_256,
};
use rand::thread_rng;

type AtomicStorage = phaselock::traits::implementations::AtomicStorage<DEntryBlock, State, H_256>;

#[async_std::test]
async fn test_happy_path_blocks() {
    // This folder will be destroyed when the last handle to it closes
    let file = tempfile::tempdir().expect("Could not create temp dir");
    let path = file.path();
    println!("Using store in {:?}", path);
    let mut store = AtomicStorage::open(path).expect("Could not open atomic store");

    let block = DEntryBlock::default();
    let hash = block.hash();
    store.insert_block(hash, block.clone()).await;
    store.commit().await.expect("Could not commit");

    // Make sure the data is still there after re-opening
    drop(store);
    store = AtomicStorage::open(path).expect("Could not open atomic store");
    assert_eq!(
        store.get_block(&hash).await.unwrap(),
        DEntryBlock::default()
    );

    // Add some transactions
    let mut rng = thread_rng();
    let state = common::get_starting_state();
    let mut hashes = Vec::new();
    let mut block = block;
    for _ in 0..10 {
        let new = block
            .add_transaction_raw(&random_transaction(&state, &mut rng))
            .expect("Could not add transaction");
        println!("Inserting {:?}: {:?}", new.hash(), new);
        store.insert_block(new.hash(), new.clone()).await.unwrap();
        hashes.push(new.hash());
        block = new;
    }

    // read them all back 3 times
    // 1st time: normal readback
    // 2nd: after a commit
    // 3rd: after dropping and re-opening the store
    for i in 0..3 {
        if i == 1 {
            store.commit().await.unwrap();
        }
        if i == 2 {
            drop(store);
            store = AtomicStorage::open(path).expect("Could not open atomic store");
        }

        // read them all back
        for (idx, hash) in hashes.iter().enumerate() {
            match store.get_block(hash).await {
                StorageResult::Some(block) => println!("read {:?}", block),
                StorageResult::None => panic!("Could not read hash {} {:?}", idx, hash),
                StorageResult::Err(e) => panic!("Could not read hash {} {:?}\n{:?}", idx, hash, e),
            }
        }
    }
}

#[async_std::test]
async fn test_happy_path_qcs() {
    // This folder will be destroyed when the last handle to it closes
    let file = tempfile::tempdir().expect("Could not create temp dir");
    let path = file.path();
    println!("Using store in {:?}", path);
    let mut store = AtomicStorage::open(path).expect("Could not open atomic store");

    // Add some certificates
    let mut certs = Vec::<QuorumCertificate<H_256>>::new();
    for i in 0..10 {
        let cert = QuorumCertificate {
            view_number: i,
            ..random_quorom_certificate()
        };
        println!("Inserting {:?}", cert);
        store.insert_qc(cert.clone()).await.unwrap();
        certs.push(cert);
    }

    // read them all back 3 times
    // 1st time: normal readback
    // 2nd: after a commit
    // 3rd: after dropping and re-opening the store
    for i in 0..3 {
        if i == 1 {
            store.commit().await.unwrap();
        }
        if i == 2 {
            drop(store);
            store = AtomicStorage::open(path).expect("Could not open atomic store");
        }

        for cert in &certs {
            match store.get_qc_for_view(cert.view_number).await {
                StorageResult::Some(c) => {
                    println!("read {:?}", c);
                    assert_eq!(&c, cert);
                }
                StorageResult::None => panic!(
                    "Could not read view_number {}: {:?}",
                    cert.view_number, cert
                ),
                StorageResult::Err(e) => panic!(
                    "Could not read view_number {} {:?}\n{:?}",
                    cert.view_number, cert, e
                ),
            }
            match store.get_qc(&cert.block_hash).await {
                StorageResult::Some(c) => {
                    println!("read {:?}", c);
                    assert_eq!(&c, cert);
                }
                StorageResult::None => panic!(
                    "Could not read block_hash {:?}: {:?}",
                    cert.block_hash, cert
                ),
                StorageResult::Err(e) => panic!(
                    "Could not read block_hash {:?} {:?}\n{:?}",
                    cert.block_hash, cert, e
                ),
            }
        }
    }
}

#[async_std::test]
async fn test_happy_path_leaves() {
    // This folder will be destroyed when the last handle to it closes
    let file = tempfile::tempdir().expect("Could not create temp dir");
    let path = file.path();
    println!("Using store in {:?}", path);
    let mut store = AtomicStorage::open(path).expect("Could not open atomic store");

    // Add some leaves
    let mut leaves = Vec::<Leaf<DEntryBlock, H_256>>::new();
    for _ in 0..10 {
        let leaf = random_leaf(DEntryBlock {
            previous_block: StateHash::random(),
            ..Default::default()
        });
        println!("Inserting {:?}", leaf);
        store.insert_leaf(leaf.clone()).await.unwrap();
        leaves.push(leaf);
    }

    // read them all back 3 times
    // 1st time: normal readback
    // 2nd: after a commit
    // 3rd: after dropping and re-opening the store
    for i in 0..3 {
        if i == 1 {
            store.commit().await.unwrap();
        }
        if i == 2 {
            drop(store);
            store = AtomicStorage::open(path).expect("Could not open atomic store");
        }

        for leaf in &leaves {
            match store.get_leaf(&leaf.hash()).await {
                StorageResult::Some(l) => {
                    println!("read {:?}", l);
                    assert_eq!(&l, leaf);
                }
                StorageResult::None => {
                    panic!("Could not read leaf hash {:?}: {:?}", leaf.hash(), leaf)
                }
                StorageResult::Err(e) => panic!(
                    "Could not read leaf hash {:?} {:?}\n{:?}",
                    leaf.hash(),
                    leaf,
                    e
                ),
            }
            let hash = BlockContents::hash(&leaf.item);
            match store.get_leaf_by_block(&hash).await {
                StorageResult::Some(l) => {
                    println!("read {:?}", l);
                    assert_eq!(&l, leaf);
                }
                StorageResult::None => panic!("Could not read leaf hash {:?}: {:?}", hash, leaf),
                StorageResult::Err(e) => {
                    panic!("Could not read leaf hash {:?} {:?}\n{:?}", hash, leaf, e)
                }
            }
        }
    }
}
