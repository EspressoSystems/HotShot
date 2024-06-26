//! This module defines the interaction layer with the Solver via the [`AuctionResults`] trait,
//! which handles connecting to, and fetching the allocation results from, the Solver.

use super::node_implementation::NodeType;
use anyhow::Result;
use async_trait::async_trait;
use url::Url;

/// This trait guarantees that a particular type has a builder url associated with it. It is meant
/// to be used with the AuctionSolverResult within [`AuctionResults`] to ensure that the returned
/// data indeed points to a valid builder.
pub trait HasBuilderUrl {
    /// Returns the builer url associated with the datatype
    fn builder_url(&self) -> Url;
}

/// The AuctionResults trait is the sole source of Solver-originated allocations, and returns
/// the results via its own custom type.
#[async_trait]
pub trait AuctionResults<TYPES: NodeType>: Send + Sync {
    /// The AuctionSolverResult is a type that holds the data associated with a particular solver
    /// run, for a particular view.
    type AuctionSolverResult: HasBuilderUrl;

    /// Fetches the auction result for a view. Does not cache the result,
    /// subsequent calls will invoke additional wasted calls.
    async fn fetch_auction_result(
        self,
        view_number: TYPES::Time,
    ) -> Result<Vec<Self::AuctionSolverResult>>;
}
