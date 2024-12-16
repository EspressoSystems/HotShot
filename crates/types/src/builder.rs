/// Options controlling how the random builder generates blocks
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct RandomBuilderConfig {
    /// How many transactions to include in a block
    pub txn_in_block: u64,
    /// How many blocks to generate per second
    pub blocks_per_second: u32,
    /// Range of how big a transaction can be (in bytes)
    pub txn_size: std::ops::Range<u32>,
}

impl Default for RandomBuilderConfig {
    fn default() -> Self {
        Self {
            txn_in_block: 100,
            blocks_per_second: 1,
            txn_size: 20..100,
        }
    }
}
