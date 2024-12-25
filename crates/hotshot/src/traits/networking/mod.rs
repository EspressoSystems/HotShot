//! Networking module for HotShot consensus protocol

pub mod request_response;

// Re-exporting the main types for convenience.
pub use request_response::{
    NetworkRequest,
    NetworkResponse,
    RequestHandler,
    RequestType,
    ResponseStatus,
};
