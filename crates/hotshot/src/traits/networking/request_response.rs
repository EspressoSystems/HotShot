use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum RequestType {
    LeafByHeight(u64),
    PayloadByVidCommitment(Vec<u8>),
    VidCommonByCommitment(Vec<u8>), 
    FeeAccountStates,
    BlocksFrontier,
    Config,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkRequest {
    pub request_type: RequestType,
    pub requester_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NetworkResponse {
    pub data: Vec<u8>,
    pub status: ResponseStatus,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ResponseStatus {
    Success,
    NotFound,
    Error(String),
}

#[async_trait]
pub trait RequestHandler {
    async fn handle_request(&self, request: NetworkRequest) -> NetworkResponse;
}
