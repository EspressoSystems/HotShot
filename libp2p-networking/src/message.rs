use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct Message {
    pub sender: String,
    pub content: String,
    pub topic: String,
}
