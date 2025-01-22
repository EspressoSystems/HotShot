use std::fmt::Debug;

use super::Serializable;

/// A trait for a request. Associates itself with a response type.
pub trait Request: Send + Sync + Serializable + 'static + PartialEq + Eq + Clone + Debug {
    /// The response type associated with this request
    type Response: Response<Self>;

    /// Validate the request
    fn is_valid(&self) -> bool;
}

/// A trait that a response needs to implement
pub trait Response<R: Request>:
    Send + Sync + Serializable + PartialEq + Eq + Clone + Debug
{
    /// Validate the response, making sure it is valid for the given request
    fn is_valid(&self, request: &R) -> bool;
}
