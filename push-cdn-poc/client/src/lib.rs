mod connection;
mod testing;

use std::{hash::Hash, net::SocketAddr, sync::Arc};

use connection::InfallibleConnection;
use derive_more::Deref;
use jf_primitives::signatures::bls_over_bn254::{SignKey, VerKey};
use quinn::{ClientConfig, Endpoint};
use testing::SkipServerVerification;

use common::{
    err,
    error::{BrokerError, Result},
    stream::Connection,
    topic::Topic,
};
use tokio::sync::OnceCell;

#[derive(Clone)]
pub struct Config {
    pub broker_address: String,
    pub initial_topics: Vec<Topic>,
    pub signing_key: SignKey,
    pub verify_key: VerKey,
}

#[derive(Clone, Deref)]
pub struct Client(Arc<InfallibleConnection>);

impl Client {
    pub fn from_connection(
        endpoint: Endpoint,
        connection: OnceCell<Connection>,
        broker_address: SocketAddr,
        initial_topics: Vec<Topic>,
        signing_key: SignKey,
        verify_key: VerKey,
    ) -> Result<Self> {
        Ok(Client(Arc::new(InfallibleConnection::from_connection(
            connection,
            endpoint,
            broker_address,
            initial_topics,
            signing_key,
            verify_key,
        ))))
    }
}

impl PartialEq for Client {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Client {}

impl Hash for Client {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.hash(state)
    }
}

impl Client {
    pub fn new(config: Config) -> Result<Self> {
        // extract config values
        let Config {
            broker_address,
            initial_topics,
            signing_key,
            verify_key,
        } = config;

        // parse all broker addresses
        let broker_address = err!(
            broker_address.parse(),
            ParseError,
            "failed to parse broker address"
        )?;

        // create client endpoint by binding to local address
        let mut endpoint = err!(
            Endpoint::client("0.0.0.0:0".parse().unwrap()),
            BindError,
            "failed to bind to local address"
        )?;

        // set up TLS configuration
        #[cfg(not(feature = "local-testing"))]
        // production mode: native certs
        let config = ClientConfig::with_native_roots();

        // local testing mode: skip server verification, insecure
        #[cfg(feature = "local-testing")]
        let config = ClientConfig::new(SkipServerVerification::new_config());

        endpoint.set_default_client_config(config);

        Self::from_connection(
            endpoint,
            OnceCell::new(),
            broker_address,
            initial_topics,
            signing_key,
            verify_key,
        )
    }
}
