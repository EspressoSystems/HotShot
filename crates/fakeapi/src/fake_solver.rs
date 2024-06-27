use anyhow::Result;
use tide_disco::{
    api::ApiError,
    error::ServerError,
    method::{ReadState, WriteState},
    Api, App, RequestError,
    RequestParams,
};
use hotshot_types::{constants::Base, traits::signature_key::SignatureKey, PeerConfig};
use vbs::{
    version::{StaticVersion, StaticVersionType},
    BinarySerializer,
};

pub fn define_api<KEY, State, VER>() -> Result<Api<State, ServerError, VER>, ApiError>
where
    State: 'static + Send + Sync + ReadState + WriteState,
    <State as ReadState>::State: Send + Sync + OrchestratorApi<KEY>,
    KEY: serde::Serialize + SignatureKey,
    VER: StaticVersionType + 'static,
{
    let api_toml = toml::from_str::<toml::Value>(
        include_str!(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/apis",
                "/solver.toml"
                )
            )
        ).expect("API file is not valid toml");

    let mut api = Api::<State, ServerError, VER>::new(api_toml)?;
    api.get("allocation_result", |req, state| {
        async move {
        }.boxed()
    })?;
}
