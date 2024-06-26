use anyhow::{bail, Result};
use async_trait::async_trait;
use hotshot_types::traits::{
    auction_results::{AuctionResults, HasBuilderUrl},
    node_implementation::NodeType,
};
use url::Url;

#[derive(Debug)]
pub struct TestAuctionSolverResult {
    /// The URL of the builder to reach out to.
    pub url: Url,
}

impl HasBuilderUrl for TestAuctionSolverResult {
    fn builder_url(&self) -> Url {
        self.url.clone()
    }
}

#[derive(Debug, Default)]
pub struct TestAuctionResults {
    /// We intentionally allow for the results to be pre-cooked for the unit test to gurantee a
    /// particular outcome is met.
    pub solver_results: Vec<TestAuctionSolverResult>,

    /// A canned type to ensure that an error is thrown in absence of a true fault-injectible
    /// system for logical tests. This will guarantee that `fetch_auction_result` always throws an
    /// error.
    pub should_return_err: bool,
}

#[async_trait]
impl<TYPES: NodeType> AuctionResults<TYPES> for TestAuctionResults {
    type AuctionSolverResult = TestAuctionSolverResult;

    async fn fetch_auction_result(
        self,
        _view_number: TYPES::Time,
    ) -> Result<Vec<Self::AuctionSolverResult>> {
        if self.should_return_err {
            bail!("Something went wrong")
        }

        // Otherwise, return our pre-made results
        Ok(self.solver_results)
    }
}
