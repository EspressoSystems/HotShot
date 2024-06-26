use anyhow::{bail, Result};
use async_trait::async_trait;
use hotshot_types::traits::{
    auction_results::{AuctionResults, HasUrl},
    node_implementation::NodeType,
};
use url::Url;

/// A mock result for the auction solver. This type is just a pointer to a URL.
#[derive(Debug)]
pub struct TestAuctionSolverResult {
    /// The URL of the builder to reach out to.
    pub url: Url,
}

impl HasUrl for TestAuctionSolverResult {
    fn url(&self) -> Url {
        self.url.clone()
    }
}

/// The test auction results type is used to mimic the results from the Solver.
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

    /// Mock fetching the auction results, with optional error injection to simulate failure cases
    /// in the solver.
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
