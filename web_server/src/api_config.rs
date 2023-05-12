use jf_primitives::aead::Ciphertext;
use serde::{Deserialize, Serialize};

pub const DEFAULT_WEB_SERVER_PORT: u16 = 9000;
/// How many views to keep in memory
pub const MAX_VIEWS: usize = 10;
/// How many transactions to keep in memory
pub const MAX_TXNS: usize = 10;
pub const FIRST_SECRET: &str = "first";

pub fn get_proposal_route(view_number: u64) -> String {
    format!("api/proposal/{view_number}")
}

// pub fn post_proposal_route(view_number: u64) -> String {
//     format!("api/proposal/{view_number}")
// }
pub fn post_proposal_route(view_number: u64, secret: String) -> String {
    format!("api/secret/{view_number}/{secret}")
}

pub fn get_vote_route(view_number: u64, index: u64) -> String {
    format!("api/votes/{view_number}/{index}")
}

pub fn post_vote_route(view_number: u64) -> String {
    format!("api/votes/{view_number}")
}

pub fn get_transactions_route(index: u64) -> String {
    format!("api/transactions/{index}")
}

pub fn post_transactions_route() -> String {
    "api/transactions".to_string()
}

pub fn post_staketable_route() -> String {
    "api/staketable".to_string()
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ServerKeys<KEY> {
    #[serde(with = "jf_utils::field_elem")]
    pub enc_key: jf_primitives::aead::EncKey,
    pub pub_key: KEY,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProposalWithEncSecret {
    #[serde(with = "jf_utils::field_elem")]
    pub secret: Ciphertext,
    pub proposal: Vec<u8>,
}
