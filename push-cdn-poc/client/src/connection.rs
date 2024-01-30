use std::{hash::Hash, net::SocketAddr, sync::Arc};

use chrono::Utc;
use common::{
    crypto, err,
    error::{BrokerError, Result},
    op,
    rng::DeterministicRng,
    schema::{Authenticate, Message},
    stream::{Connection, SendStream},
    topic::Topic,
};
use jf_primitives::signatures::{
    bls_over_bn254::{BLSOverBN254CurveSignatureScheme as BLS, SignKey, VerKey},
    SignatureScheme,
};

use tokio::sync::{OnceCell, RwLock};
use tracing::{info, warn};

#[derive(Clone, Default)]
struct FallibleConnection(Arc<RwLock<OnceCell<Connection>>>);

impl FallibleConnection {}

#[derive(Clone)]
pub struct InfallibleConnection {
    broker_address: SocketAddr,
    endpoint: quinn::Endpoint,
    initial_topics: Vec<Topic>,

    connection: Arc<RwLock<OnceCell<Connection>>>,

    signing_key: SignKey,
    verify_key: VerKey,
}

impl PartialEq for InfallibleConnection {
    fn eq(&self, other: &Self) -> bool {
        self.broker_address == other.broker_address
    }
}

impl Eq for InfallibleConnection {}

impl Hash for InfallibleConnection {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.broker_address.hash(state)
    }
}

impl InfallibleConnection {
    pub fn from_connection(
        connection: OnceCell<Connection>,
        endpoint: quinn::Endpoint,
        broker_address: SocketAddr,
        initial_topics: Vec<Topic>,
        signing_key: SignKey,
        verify_key: VerKey,
    ) -> Self {
        Self {
            broker_address,
            endpoint,
            initial_topics,
            connection: Arc::new(RwLock::new(connection)),
            signing_key,
            verify_key,
        }
    }

    pub fn new(
        endpoint: quinn::Endpoint,
        broker_address: SocketAddr,
        initial_topics: Vec<Topic>,
        signing_key: SignKey,
        verify_key: VerKey,
    ) -> Self {
        Self::from_connection(
            OnceCell::new(),
            endpoint,
            broker_address,
            initial_topics,
            signing_key,
            verify_key,
        )
    }

    async fn authenticate(&self, connection: &Connection) -> Result<()> {
        // get current timestamp to sign
        let timestamp = Utc::now().timestamp();

        // sign timestamp
        let signature = err!(
            BLS::sign(
                &(),
                &self.signing_key,
                timestamp.to_be_bytes(),
                &mut DeterministicRng(0),
            ),
            CryptoError,
            "failed to sign message"
        )?;

        let verify_key_bytes = crypto::serialize(&self.verify_key)?;

        let signature_bytes = crypto::serialize(&signature)?;

        // create authentication message
        let message = Arc::new(Message::Authenticate(Authenticate {
            timestamp,
            verify_key: verify_key_bytes,
            signature: signature_bytes,
            initial_topics: self.initial_topics.clone(),
        }));

        // send auth message and get response
        let response = connection.send_request(&message).await?;

        // deal with response
        match response {
            Message::AuthenticateResponse(response) => {
                if response.success {
                    Ok(())
                } else {
                    Err(BrokerError::AuthenticationError(format!(
                        "authentication rejected: {}",
                        response.reason
                    )))
                }
            }
            _ => Err(BrokerError::AuthenticationError(
                "unexpected response type".to_string(),
            )),
        }
    }

    pub async fn wait_connect(&self) {
        match self.connection.try_write() {
            Ok(mut connection_guard) => {
                {
                    let connection = loop {
                        match self.connect().await {
                            Ok(connection) => break connection,
                            Err(e) => {
                                warn!("{e}");
                                continue;
                            }
                        };
                    };

                    // write to connection guard
                    {
                        *connection_guard = OnceCell::from(connection);
                    }
                };
            }
            Err(_) => {
                let _ = self.connection.read().await;
            }
        }
    }

    async fn connect(&self) -> Result<Connection> {
        // connect to broker at given address

        let connection = Connection(err!(
            err!(
                self.endpoint
                    //TODO: change from localhost
                    .connect(self.broker_address, "localhost"),
                ConnectionError,
                "failed to connect to broker"
            )?
            .await,
            ConnectionError,
            "failed to connect to broker"
        )?);

        // attempt to authenticate conection
        err!(
            self.authenticate(&connection).await,
            AuthenticationError,
            "failed to authenticate with broker"
        )?;

        info!("successfully connected to broker");

        Ok(connection)
    }

    pub async fn send_request(&self, message: &Arc<Message>) -> Message {
        loop {
            match self.send_request_fallible(message).await {
                Ok(msg) => break msg,
                Err(e) => {
                    warn!("{e}");
                    self.wait_connect().await;
                }
            }
        }
    }
    pub async fn recv_request(&self) -> (Message, SendStream) {
        loop {
            match self.recv_request_fallible().await {
                Ok(msg) => break msg,
                Err(e) => {
                    warn!("{e}");
                    self.wait_connect().await;
                }
            }
        }
    }
    pub async fn send_message(&self, message: Arc<Message>) {
        loop {
            match self.send_message_fallible(&message).await {
                Ok(msg) => break msg,
                Err(e) => {
                    warn!("{e}");
                    self.wait_connect().await;
                }
            }
        }
    }
    pub async fn recv_message(&self) -> Message {
        loop {
            match self.recv_message_fallible().await {
                Ok(msg) => break msg,
                Err(e) => {
                    warn!("{e}");
                    self.wait_connect().await;
                }
            }
        }
    }

    async fn send_request_fallible(&self, message: &Arc<Message>) -> Result<Message> {
        err!(
            op!(
                self.connection.read().await.get(),
                ConnectionError,
                "not connected yet -> attempting to establish connection"
            )?
            .send_request(message)
            .await,
            ConnectionError,
            "failed to send request"
        )
    }
    async fn recv_request_fallible(&self) -> Result<(Message, SendStream)> {
        err!(
            op!(
                self.connection.read().await.get(),
                ConnectionError,
                "not connected yet -> attempting to establish connection"
            )?
            .recv_request()
            .await,
            ConnectionError,
            "failed to receive request"
        )
    }
    async fn send_message_fallible(&self, message: &Arc<Message>) -> Result<()> {
        err!(
            op!(
                self.connection.read().await.get(),
                ConnectionError,
                "not connected yet -> attempting to establish connection"
            )?
            .send_message(message)
            .await,
            ConnectionError,
            "failed to send message"
        )
    }
    async fn recv_message_fallible(&self) -> Result<Message> {
        err!(
            op!(
                self.connection.read().await.get(),
                ConnectionError,
                "not connected yet -> attempting to establish connection"
            )?
            .recv_message()
            .await,
            ConnectionError,
            "failed to receive message"
        )
    }
}
