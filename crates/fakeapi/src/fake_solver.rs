use anyhow::Result;
use futures::FutureExt;
use hotshot_example_types::auction_results_provider_types::TestAuctionResult;
use hotshot_types::traits::{node_implementation::NodeType, signature_key::SignatureKey};
use std::{thread, time};
use tide_disco::{
    api::ApiError,
    error::ServerError,
    method::{ReadState, WriteState},
    Api, Url,
};
use vbs::version::StaticVersionType;

const SOLVER_MAX_TIMEOUT_S: time::Duration = time::Duration::from_secs(1);

pub enum FakeSolverFaultType {
    InternalServerFault,
    TimeoutFault,
}

pub struct FakeSolverState {
    /// The rate at which an error of any kind occurs
    pub error_pct: f32,

    /// The available builder list
    pub available_builders: Vec<Url>,
}

impl FakeSolverState {
    /// Make a new `FakeSolverState` object
    #[must_use]
    pub fn new(error_pct: Option<f32>, available_builders: Vec<Url>) -> Self {
        Self {
            error_pct: error_pct.unwrap_or(0.0),
            available_builders,
        }
    }

    /// If a random fault event happens, what fault should we send?
    #[must_use]
    pub fn should_fault(&self) -> Option<FakeSolverFaultType> {
        if rand::random::<f32>() < self.error_pct {
            // Spin a random number over the fault types
            if rand::random::<f32>() < 0.5 {
                return Some(FakeSolverFaultType::InternalServerFault);
            }

            return Some(FakeSolverFaultType::TimeoutFault);
        }

        None
    }

    /// Dumps back the builders with non deterministic error if the `error_pct` field
    /// is nonzero.
    ///
    /// # Errors
    /// Returns an error if the `should_fault` method is `Some`.
    pub fn dump_builders(&self) -> Result<Vec<TestAuctionResult>, ServerError> {
        if let Some(fault) = self.should_fault() {
            match fault {
                FakeSolverFaultType::InternalServerFault => {
                    return Err(ServerError {
                        status: tide_disco::StatusCode::INTERNAL_SERVER_ERROR,
                        message: "Internal Server Error".to_string(),
                    });
                }
                FakeSolverFaultType::TimeoutFault => {
                    // Sleep for the preconfigured 1 second timeout interval
                    thread::sleep(SOLVER_MAX_TIMEOUT_S);
                }
            }
        }

        // Now just send the builder urls
        Ok(self
            .available_builders
            .iter()
            .map(|url| TestAuctionResult { url: url.clone() })
            .collect())
    }
}

#[async_trait::async_trait]
pub trait FakeSolverApi<TYPES: NodeType> {
    async fn get_auction_results_non_permissioned(
        &self,
        _view_number: u64,
    ) -> Result<Vec<TestAuctionResult>, ServerError>;
    async fn get_auction_results_permissioned(
        &self,
        _view_number: u64,
        _signature: &<TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType,
    ) -> Result<Vec<TestAuctionResult>, ServerError>;
}

#[async_trait::async_trait]
impl<TYPES: NodeType> FakeSolverApi<TYPES> for FakeSolverState {
    async fn get_auction_results_non_permissioned(
        &self,
        _view_number: u64,
    ) -> Result<Vec<TestAuctionResult>, ServerError> {
        self.dump_builders()
    }

    async fn get_auction_results_permissioned(
        &self,
        _view_number: u64,
        _signature: &<TYPES::SignatureKey as SignatureKey>::PureAssembledSignatureType,
    ) -> Result<Vec<TestAuctionResult>, ServerError> {
        self.dump_builders()
    }
}

/// Defines the API for the Fake solver.
/// # Errors
/// Returns an error if any of the initialization operations fail.
/// # Panics
/// Panics when type conversion fails.
pub fn define_api<TYPES, State, VER>() -> Result<Api<State, ServerError, VER>, ApiError>
where
    TYPES: NodeType,
    State: 'static + Send + Sync + ReadState + WriteState,
    <State as ReadState>::State: Send + Sync + FakeSolverApi<TYPES>,
    VER: StaticVersionType + 'static,
{
    let api_toml = toml::from_str::<toml::Value>(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/apis",
        "/solver.toml"
    )))
    .expect("API file is not valid toml");

    let mut api = Api::<State, ServerError, VER>::new(api_toml)?;
    api.get("get_auction_results_non_permissioned", |req, state| {
        async move {
            let view_number = req.integer_param("view_number")?;
            state
                .get_auction_results_non_permissioned(view_number)
                .await
        }
        .boxed()
    })?
    .get("get_auction_results_permissioned", |req, state| {
        async move {
            let view_number = req.integer_param("view_number")?;
            let signature = req.tagged_base64_param("signature")?;
            state
                .get_auction_results_permissioned(
                    view_number,
                    &signature
                        .try_into()
                        .map_err(|_| ServerError {
                            message: "Invalid signature".to_string(),
                            status: tide_disco::StatusCode::UNPROCESSABLE_ENTITY,
                        })
                        .unwrap(),
                )
                .await
        }
        .boxed()
    })?;
    Ok(api)
}
