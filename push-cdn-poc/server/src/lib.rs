use std::{
    collections::HashSet,
    fs::{self},
    sync::Arc,
    time::Duration,
};

use client::Client;
use discovery::{RedisClient, RedisConfig};
use jf_primitives::signatures::{
    bls_over_bn254::{BLSOverBN254CurveSignatureScheme as BLS, SignKey, VerKey},
    SignatureScheme,
};
use quinn::{Endpoint, ServerConfig, VarInt};
use rcgen::generate_simple_self_signed;

use common::{
    constant::MAX_CONCURRENT_STREAMS,
    crypto, err,
    error::{BrokerError, Result},
    op,
    schema::{AuthenticateResponse, Message},
    stream::Connection,
    topic::Topic,
};
use state::ConnectionMap;
use tokio::{
    spawn,
    sync::{OnceCell, RwLock},
    task::JoinHandle,
    time::sleep,
};
use tracing::{error, info, warn};

mod discovery;
mod state;

pub struct Config {
    pub bind_address: String,
    pub public_advertise_address: String,
    pub private_advertise_address: String,

    pub signing_key: Option<SignKey>,

    pub redis_url: String,
    pub redis_password: String,

    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
}
pub struct Server {
    endpoint: Endpoint,

    signing_key: SignKey,
    verify_key: VerKey,

    redis_client: RedisClient,
    register_task_handle: Option<JoinHandle<()>>,

    user_map: Arc<RwLock<ConnectionMap<VerKey, Connection>>>,
    broker_map: Arc<RwLock<ConnectionMap<VerKey, Client>>>,
}

impl Server {
    pub fn new(config: Config) -> Result<Self> {
        // extract config
        let Config {
            bind_address,
            public_advertise_address,
            private_advertise_address,
            signing_key,
            redis_url,
            redis_password,
            tls_cert_path,
            tls_key_path,
        } = config;

        // load certificates
        let (cert_chain, priv_key) = Self::load_certificates(tls_cert_path, tls_key_path)?;

        // create server config
        let server_config = err!(
            ServerConfig::with_single_cert(cert_chain, priv_key),
            CryptoError,
            "failed to use certificate"
        )?;

        // parse addresses
        let bind_address = err!(
            bind_address.parse(),
            ParseError,
            "failed to parse bind address"
        )?;
        let public_advertise_address = err!(
            public_advertise_address.parse(),
            ParseError,
            "failed to parse public advertise address"
        )?;
        let private_advertise_address = err!(
            private_advertise_address.parse(),
            ParseError,
            "failed to parse private advertise address"
        )?;

        // create endpoint
        let endpoint = err!(
            Endpoint::server(server_config, bind_address),
            BindError,
            "failed to bind to address"
        )?;

        // create redis client from redis endpoint and advertise address
        let redis_client = err!(
            RedisClient::new(RedisConfig {
                redis_url,
                redis_password,
                public_advertise_address,
                private_advertise_address,
                // expiry is 10 minutes
                record_expiry: Duration::from_secs(60 * 10),
            }),
            RedisError,
            "failed to create Redis client"
        )?;

        // deserialize broker key from hex, or randomly generate one (for single broker-only mode)
        let signing_key = signing_key.unwrap_or(crypto::random_key::<BLS>()?);

        let verify_key = VerKey::from(&signing_key);
        info!("broker has key {}", verify_key);

        Ok(Self {
            endpoint,
            redis_client,
            signing_key,
            verify_key,
            register_task_handle: None,
            user_map: Arc::default(),
            broker_map: Arc::default(),
        })
    }

    pub async fn handle_broker_connection(
        user_connections: &Arc<RwLock<ConnectionMap<VerKey, Connection>>>,
        broker_connections: &Arc<RwLock<ConnectionMap<VerKey, Client>>>,
        signing_key: SignKey,
        verify_key: VerKey,
        endpoint: Endpoint,
        connection: Connection,
    ) -> Result<()> {
        // wait for the broker to advertise its address to us
        let private_address = if let Message::Identity(address) = err!(
            connection.recv_message().await,
            ReadError,
            "failed to receive message from broker"
        )? {
            address
        } else {
            return Err(BrokerError::AuthenticationError(
                "broker failed to authenticate".to_string(),
            ));
        };

        // create client from connection
        let client = Client::from_connection(
            endpoint,
            OnceCell::from(connection),
            private_address,
            vec![],
            signing_key,
            verify_key,
        )?;

        // add client to map
        broker_connections
            .write()
            .await
            .add_connection(verify_key, client, HashSet::new());

        Ok(())
    }

    pub async fn handle_user_connection(
        user_connections: &Arc<RwLock<ConnectionMap<VerKey, Connection>>>,
        verify_key: VerKey,
        connection: Connection,
        initial_topics: HashSet<Topic>,
    ) -> Result<()> {
        // add connection to our map, closing pre-existing ones
        if let Some(conn) = user_connections.write().await.add_connection(
            verify_key,
            connection.clone(),
            initial_topics,
        ) {
            conn.close(0u32.into(), b"");
        };

        // receive requests and messages
        loop {
            let message = err!(
                connection.recv_message().await,
                ConnectionError,
                "failed to receive message from user"
            )?;

            // handle message
            match message {
                Message::Broadcast(topics, data) => {
                    // arc message
                    let message = Arc::new(Message::Data(data));

                    // look up subscribed parties for the broadcast
                    let subscribed_connections = user_connections
                        .read()
                        .await
                        .get_connections_for_broadcast(verify_key, topics);

                    for connection in subscribed_connections {
                        let message = message.clone();
                        spawn(async move {
                            if let Err(e) = connection.send_message(&message).await {
                                warn!("failed to send message: {e}");
                            };
                        });
                    }
                }

                Message::Direct(key, data) => {
                    // arc message
                    let message = Arc::new(Message::Data(data));

                    // deserialize signing key
                    let verify_key = match crypto::deserialize(key) {
                        Ok(verify_key) => verify_key,
                        Err(e) => {
                            warn!("failed to deserialize verify key: {e}");
                            continue;
                        }
                    };

                    if let Some(connection) = user_connections
                        .read()
                        .await
                        .get_connection_for_direct(verify_key)
                    {
                        spawn(async move {
                            if let Err(e) = connection.send_message(&message).await {
                                warn!("failed to send message: {e}");
                            };
                        });
                    }
                }

                Message::Subscribe(subscribe) => {
                    user_connections
                        .write()
                        .await
                        .subscribe_key_to(verify_key, subscribe.topics);
                }

                Message::Unsubscribe(unsubscribe) => {
                    user_connections
                        .write()
                        .await
                        .unsubscribe_key_from(verify_key, unsubscribe.topics);
                }

                _ => {}
            }
        }
    }

    pub async fn run(mut self) -> Result<()> {
        // register with Redis, start async task
        self.start_register_task(Duration::from_secs(20 * 60)).await;

        // main loop
        loop {
            // receive new connections
            let mut connection = match self.accept().await {
                Ok(connection) => connection,
                Err(e) => {
                    warn!("failed to open connection: {e}");
                    continue;
                }
            };

            // clone maps we may need
            // TODO: see if we can optimize this, MEASURE IT
            let user_map = self.user_map.clone();
            let broker_map = self.broker_map.clone();
            let endpoint = self.endpoint.clone();

            // clone keys
            let verify_key = self.verify_key.clone();
            let signing_key = self.signing_key.clone();

            // spawn new task to handle connection in background
            spawn(async move {
                // authenticate the connection
                let (verify_key, initial_topics) = match Self::authenticate(&mut connection).await {
                    Ok(verify_key) => verify_key,
                    Err(e) => {
                        warn!("failed to authenticate connection: {e}");
                        return;
                    }
                };

                if verify_key == self.verify_key {
                    // broker, deal with broker connection
                    if let Err(e) = err!(
                        Self::handle_broker_connection(
                            &user_map,
                            &broker_map,
                            signing_key,
                            verify_key,
                            endpoint,
                            connection
                        )
                        .await,
                        ConnectionError,
                        "connection to broker failed"
                    ) {
                        // we care more about broker<->broker connections
                        error!("{e}");
                    };
                } else {
                    // user, deal with user connection
                    let error = err!(
                        Self::handle_user_connection(
                            &user_map,
                            verify_key,
                            connection,
                            initial_topics
                        )
                        .await,
                        ConnectionError,
                        "connection to user failed"
                    );

                    // remove connection from map
                    if let Some(conn) = user_map.write().await.remove_connection(verify_key) {
                        conn.close(0u32.into(), b"");
                    };

                    // log error
                    if let Err(e) = error {
                        warn!("{e}");
                    }
                }
            });
        }
    }

    pub async fn accept(&mut self) -> Result<Connection> {
        // accept connection
        let connection = Connection(err!(
            op!(
                self.endpoint.accept().await,
                ConnectionError,
                "endpoint is closed"
            )?
            .await,
            ConnectionError,
            "failed to accept connection"
        )?);

        connection.set_max_concurrent_bi_streams(VarInt::from_u64(MAX_CONCURRENT_STREAMS).unwrap());
        connection
            .set_max_concurrent_uni_streams(VarInt::from_u64(MAX_CONCURRENT_STREAMS).unwrap());

        Ok(connection)
    }

    pub async fn authenticate(connection: &mut Connection) -> Result<(VerKey, HashSet<Topic>)> {
        // accept bidirectional stream
        let (message, send_stream) = connection.recv_request().await?;

        // process request , send response
        let (response, verify_key, topics) = Self::handle_auth_request(message);
        if !response.success {
            return Err(BrokerError::AuthenticationError(format!(
                "client failed to authenticate: {}",
                response.reason
            )));
        }

        send_stream
            .send_message(&Arc::new(Message::AuthenticateResponse(response)))
            .await?;

        Ok((verify_key.unwrap(), topics))
    }

    fn handle_auth_request(
        message: Message,
    ) -> (AuthenticateResponse, Option<VerKey>, HashSet<Topic>) {
        // see if we're the right type of message
        let Message::Authenticate(message) = message else {
            return (
                AuthenticateResponse {
                    success: false,
                    reason: "unexpected message type".to_string(),
                },
                None,
                HashSet::new(),
            );
        };

        // deserialize verify key
        let Ok(verify_key) = crypto::deserialize(message.verify_key) else {
            return (
                AuthenticateResponse {
                    success: false,
                    reason: "malformed verify key".to_string(),
                },
                None,
                HashSet::new(),
            );
        };

        // deserialize signature
        let Ok(signature) = crypto::deserialize(message.signature) else {
            return (
                AuthenticateResponse {
                    success: false,
                    reason: "malformed signature".to_string(),
                },
                None,
                HashSet::new(),
            );
        };

        // verify signature
        if BLS::verify(
            &(),
            &verify_key,
            message.timestamp.to_be_bytes(),
            &signature,
        )
        .is_err()
        {
            return (
                AuthenticateResponse {
                    success: false,
                    reason: "verification failed".to_string(),
                },
                None,
                HashSet::new(),
            );
        }

        // make sure timestamp is within 5 seconds
        let timestamp = chrono::Utc::now().timestamp();
        if timestamp - message.timestamp > 5 {
            return (
                AuthenticateResponse {
                    success: false,
                    reason: "timestamp is too old".to_string(),
                },
                None,
                HashSet::new(),
            );
        }

        (
            AuthenticateResponse {
                success: true,
                reason: "".to_string(),
            },
            Some(verify_key),
            HashSet::from_iter(message.initial_topics),
        )
    }

    async fn start_register_task(&mut self, interval: Duration) {
        let redis_client = self.redis_client.clone();
        let task_handle = spawn(async move {
            // register
            if let Err(e) = redis_client.register().await {
                warn!("failed to register with Redis: {e}");

                // sleep 10s to avoid overloading redis
                sleep(Duration::from_secs(10)).await;
            };

            // sleep for duration
            sleep(interval).await;
        });

        self.register_task_handle = Some(task_handle);
    }

    fn load_certificates(
        cert_path: Option<String>,
        key_path: Option<String>,
    ) -> Result<(Vec<rustls::Certificate>, rustls::PrivateKey)> {
        // conditionally generate or load certificate
        match (cert_path, key_path) {
            (Some(cert_path), Some(key_path)) => {
                // load cert in from file
                let cert = pem::parse(fs::read(cert_path).map_err(|err| {
                    BrokerError::FileError(format!("failed to read certificate file: {err}",))
                })?)
                .map_err(|err| {
                    BrokerError::ParseError(
                        format!("failed to parse certificate from file: {err}",),
                    )
                })?
                .into_contents();

                // load key in from file
                let key = pem::parse(fs::read(key_path).map_err(|err| {
                    BrokerError::FileError(format!("failed to read key file: {err}"))
                })?)
                .map_err(|err| {
                    BrokerError::ParseError(format!("failed to parse key from file: {err}",))
                })?
                .into_contents();

                // convert cert and key to rustls types
                let cert = vec![rustls::Certificate(cert)];
                let key = rustls::PrivateKey(key);

                Ok((cert, key))
            }
            (_, _) => {
                // generate localhost cert with rcgen
                let cert = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
                let cert_der = cert.serialize_der().unwrap();
                let key_der = rustls::PrivateKey(cert.serialize_private_key_der());
                Ok((vec![rustls::Certificate(cert_der)], key_der))
            }
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(task_handle) = &self.register_task_handle {
            task_handle.abort();
        }
    }
}
