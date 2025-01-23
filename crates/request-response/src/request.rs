use std::{fmt::Debug, hash::Hash};

use anyhow::Result;

use super::Serializable;

/// A trait for a request. Associates itself with a response type.
pub trait Request:
    Send + Sync + Serializable + 'static + PartialEq + Eq + Clone + Debug + Hash
{
    /// The response type associated with this request
    type Response: Response<Self>;

    /// Validate the request, returning an error if it is not valid
    ///
    /// # Errors
    /// If the request is not valid
    fn validate(&self) -> Result<()>;
}

/// A trait that a response needs to implement
pub trait Response<R: Request>:
    Send + Sync + Serializable + PartialEq + Eq + Clone + Debug
{
    /// Validate the response, making sure it is valid for the given request
    ///
    /// # Errors
    /// If the response is not valid for the given request
    fn validate(&self, request: &R) -> Result<()>;
}
