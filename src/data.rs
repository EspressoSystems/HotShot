use serde::{Deserialize, Serialize};

use crate::{BlockContents, BlockHash};

#[derive(Serialize, Deserialize, Clone)]
pub struct Leaf<T> {
    pub parent: BlockHash,
    pub item: T,
}

impl<T: BlockContents> Leaf<T> {
    pub fn new(item: T, parent: BlockHash) -> Self {
        Leaf { item, parent }
    }

    pub fn hash(&self) -> BlockHash {
        todo!();
    }
}
