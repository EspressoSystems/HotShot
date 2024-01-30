use rkyv::{Archive, Deserialize, Serialize};

#[derive(Archive, Deserialize, Serialize, Debug, PartialEq, Clone, Hash, Eq)]
#[archive(check_bytes)]
pub enum Topic {
    Global,
    DA,
}
