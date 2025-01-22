use anyhow::Result;
use async_trait::async_trait;

use super::request::Request;

/// The trait that allows the [`RequestResponseProtocol`] to calculate/derive a response for a specific request
#[async_trait]
pub trait DataSource<R: Request>: Send + Sync + 'static {
    /// Calculate/derive the response data for a specific request
    async fn derive_response_data(&self, request: &R) -> Result<R::Response>;
}
