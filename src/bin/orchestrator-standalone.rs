use hotshot::{
    traits::election::static_committee::StaticElectionConfig,
    types::{
        ed25519::{Ed25519Priv, Ed25519Pub},
        SignatureKey,
    },
};
use hotshot_orchestrator;

#[async_std::main]
pub async fn main() {
    hotshot_orchestrator::run_orchestrator::<Ed25519Pub, StaticElectionConfig>().await;
}
