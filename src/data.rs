use serde::{Deserialize, Serialize};

use crate::{BlockContents, BlockHash};

#[derive(Serialize, Deserialize, Clone, Debug)]
/// A node in `HotStuff`'s tree
pub struct Leaf<T> {
    /// The hash of the parent
    pub parent: BlockHash,
    /// The item in the node
    pub item: T,
}

impl<T: BlockContents> Leaf<T> {
    /// Creates a new leaf with the specified contents
    pub fn new(item: T, parent: BlockHash) -> Self {
        Leaf { parent, item }
    }

    /// Hashes the leaf with Blake3
    pub fn hash(&self) -> BlockHash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.parent);
        hasher.update(&BlockContents::hash(&self.item));
        *hasher.finalize().as_bytes()
    }
}
