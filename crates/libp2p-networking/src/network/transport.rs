use anyhow::Context;
use anyhow::Result as AnyhowResult;
use async_compatibility_layer::art::async_timeout;
use futures::AsyncRead;
use futures::AsyncWrite;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;
use tracing::warn;

use futures::future::poll_fn;
use futures::{AsyncReadExt, AsyncWriteExt};
use hotshot_types::traits::signature_key::SignatureKey;
use libp2p::core::muxing::StreamMuxerExt;
use libp2p::core::transport::TransportEvent;
use libp2p::core::StreamMuxer;
use libp2p::identity::PeerId;
use libp2p::Transport;
use pin_project::pin_project;

/// The maximum size of an authentication message. This is used to prevent
/// DoS attacks by sending large messages.
const MAX_AUTH_MESSAGE_SIZE: usize = 1024;

/// The timeout for the authentication handshake. This is used to prevent
/// attacks that keep connections open indefinitely by half-finishing the
/// handshake.
const AUTH_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// A wrapper for a `Transport` that bidirectionally authenticates connections
/// by performing a handshake that checks if the remote peer is present in the
/// stake table.
#[pin_project]
pub struct StakeTableAuthentication<T: Transport, S: SignatureKey, C: StreamMuxer + Unpin> {
    #[pin]
    /// The underlying transport we are wrapping
    pub inner: T,

    /// The stake table we check against to authenticate connections
    pub stake_table: Arc<Option<HashSet<S>>>,

    /// A pre-signed message that we send to the remote peer for authentication
    pub auth_message: Arc<Option<Vec<u8>>>,

    /// Phantom data for the connection type
    pd: std::marker::PhantomData<C>,
}

impl<T: Transport, S: SignatureKey, C: StreamMuxer + Unpin> StakeTableAuthentication<T, S, C> {
    /// Create a new `StakeTableAuthentication` transport that wraps the given transport
    /// and authenticates connections against the stake table.
    pub fn new(inner: T, stake_table: Option<HashSet<S>>, auth_message: Option<Vec<u8>>) -> Self {
        Self {
            inner,
            stake_table: Arc::from(stake_table),
            auth_message: Arc::from(auth_message),
            pd: std::marker::PhantomData,
        }
    }

    /// Prove to the remote peer that we are in the stake table by sending
    /// them our authentication message.
    ///
    /// # Errors
    /// - If we fail to write the message to the stream
    pub async fn authenticate_with_remote_peer(
        stream: &mut C::Substream,
        auth_message: Arc<Option<Vec<u8>>>,
    ) -> AnyhowResult<()>
    where
        C::Substream: Unpin,
    {
        // If we have an auth message, send it to the remote peer, prefixed with
        // the message length
        if let Some(auth_message) = auth_message.as_ref() {
            // Write the length-delimited message
            write_length_delimited(stream, auth_message).await?;
        }

        Ok(())
    }

    /// Verify that the remote peer is in the stake table by checking their
    /// authentication message.
    ///
    /// # Errors
    /// If the peer fails verification. This can happen if:
    /// - We fail to read the message from the stream
    /// - The message is too large
    /// - The message is invalid
    /// - The peer is not in the stake table
    /// - The signature is invalid
    pub async fn verify_peer_authentication(
        stream: &mut C::Substream,
        stake_table: Arc<Option<HashSet<S>>>,
    ) -> AnyhowResult<()>
    where
        C::Substream: Unpin,
    {
        // Read the length-delimited message from the remote peer
        let message = read_length_delimited(stream, MAX_AUTH_MESSAGE_SIZE).await?;

        // If we have a stake table, check if the remote peer is in it
        if let Some(stake_table) = stake_table.as_ref() {
            // Deserialize the authentication message
            let auth_message: AuthMessage<S> = bincode::deserialize(&message)
                .with_context(|| "Failed to deserialize auth message")?;

            // Verify the signature on the public key
            let public_key = auth_message
                .validate()
                .with_context(|| "Failed to verify authentication message")?;

            // Check if the public key is in the stake table
            if !stake_table.contains(&public_key) {
                return Err(anyhow::anyhow!("Peer not in stake table"));
            }
        }

        Ok(())
    }
}

/// The deserialized form of an authentication message that is sent to the remote peer
#[derive(Clone, Serialize, Deserialize)]
struct AuthMessage<S: SignatureKey> {
    /// The encoded public key of the sender. This is what gets signed, but
    /// it is still encoded here so we can verify the signature.
    pub_key_bytes: Vec<u8>,

    /// The signature on the public key
    signature: S::PureAssembledSignatureType,
}

impl<S: SignatureKey> AuthMessage<S> {
    /// Validate the signature on the public key and return it if valid
    pub fn validate(&self) -> AnyhowResult<S> {
        // Deserialize the public key
        let public_key = S::from_bytes(&self.pub_key_bytes)
            .with_context(|| "Failed to deserialize public key")?;

        // Check if the signature is valid
        if !public_key.validate(&self.signature, &self.pub_key_bytes) {
            return Err(anyhow::anyhow!("Invalid signature"));
        }

        Ok(public_key)
    }
}

/// Create an sign an authentication message to be sent to the remote peer
///
/// # Errors
/// - If we fail to sign the public key
/// - If we fail to serialize the authentication message
pub fn construct_auth_message<S: SignatureKey + 'static>(
    public_key: &S,
    private_key: &S::PrivateKey,
) -> AnyhowResult<Vec<u8>> {
    // Serialize the public key
    let pub_key_bytes = public_key.to_bytes();

    // Sign our public key
    let signature =
        S::sign(private_key, &pub_key_bytes).with_context(|| "Failed to sign public key")?;

    // Create the auth message
    let auth_message = AuthMessage::<S> {
        pub_key_bytes,
        signature,
    };

    // Serialize the auth message
    bincode::serialize(&auth_message).with_context(|| "Failed to serialize auth message")
}

/// A helper function to read a length-delimited message from a stream. Takes into
/// account the maximum message size.
///
/// # Errors
/// - If the message is too big
/// - If we fail to read from the stream
pub async fn read_length_delimited<S: AsyncRead + Unpin>(
    stream: &mut S,
    max_size: usize,
) -> AnyhowResult<Vec<u8>> {
    // Receive the first 8 bytes of the message, which is the length
    let mut len_bytes = [0u8; 4];
    stream
        .read_exact(&mut len_bytes)
        .await
        .with_context(|| "Failed to read message length")?;

    // Parse the length of the message as a `u32`
    let len = usize::try_from(u32::from_be_bytes(len_bytes))?;

    // Quit if the message is too large
    if len > max_size {
        return Err(anyhow::anyhow!("Message too large"));
    }

    // Read the actual message
    let mut message = vec![0u8; len];
    stream
        .read_exact(&mut message)
        .await
        .with_context(|| "Failed to read message")?;

    Ok(message)
}

/// A helper function to write a length-delimited message to a stream.
///
/// # Errors
/// - If we fail to write to the stream
pub async fn write_length_delimited<S: AsyncWrite + Unpin>(
    stream: &mut S,
    message: &[u8],
) -> AnyhowResult<()> {
    // Write the length of the message
    stream
        .write_all(&u32::try_from(message.len())?.to_be_bytes())
        .await
        .with_context(|| "Failed to write message length")?;

    // Write the actual message
    stream
        .write_all(message)
        .await
        .with_context(|| "Failed to write message")?;

    Ok(())
}

impl<T: Transport, S: SignatureKey + 'static, C: StreamMuxer + Unpin> Transport
    for StakeTableAuthentication<T, S, C>
where
    T::Dial: Future<Output = Result<T::Output, T::Error>> + Send + 'static,
    T::ListenerUpgrade: Send + 'static,
    T::Output: AsConnection<C> + Send,
    T::Error: From<<C as StreamMuxer>::Error> + From<std::io::Error>,

    C::Substream: Unpin + Send,
{
    // `Dial` is for connecting out, `ListenerUpgrade` is for accepting incoming connections
    type Dial = Pin<Box<dyn Future<Output = Result<T::Output, T::Error>> + Send>>;
    type ListenerUpgrade = Pin<Box<dyn Future<Output = Result<T::Output, T::Error>> + Send>>;

    // These are just passed through
    type Output = T::Output;
    type Error = T::Error;

    /// Dial a remote peer. This function is changed to perform an authentication handshake
    /// on top.
    fn dial(
        &mut self,
        addr: libp2p::Multiaddr,
    ) -> Result<Self::Dial, libp2p::TransportError<Self::Error>> {
        // Perform the inner dial
        let res = self.inner.dial(addr);

        // Clone the necessary fields
        let auth_message = Arc::clone(&self.auth_message);
        let stake_table = Arc::clone(&self.stake_table);

        // If the dial was successful, perform the authentication handshake on top
        match res {
            Ok(dial) => Ok(Box::pin(async move {
                // Perform the inner dial
                let mut stream = dial.await?;

                // Time out the authentication block
                async_timeout(AUTH_HANDSHAKE_TIMEOUT, async {
                    // Open a substream for the handshake
                    let mut substream =
                        poll_fn(|cx| stream.as_connection().poll_outbound_unpin(cx)).await?;

                    // (outbound) Authenticate with the remote peer
                    Self::authenticate_with_remote_peer(&mut substream, auth_message)
                        .await
                        .map_err(|e| {
                            warn!("Failed to authenticate with remote peer: {:?}", e);
                            std::io::Error::new(std::io::ErrorKind::Other, e)
                        })?;

                    // (inbound) Verify the remote peer's authentication
                    Self::verify_peer_authentication(&mut substream, stake_table)
                        .await
                        .map_err(|e| {
                            warn!("Failed to verify remote peer: {:?}", e);
                            std::io::Error::new(std::io::ErrorKind::Other, e)
                        })?;

                    Ok::<(), T::Error>(())
                })
                .await
                .map_err(|e| {
                    warn!("Timed out during authentication handshake: {:?}", e);
                    std::io::Error::new(std::io::ErrorKind::TimedOut, e)
                })??;

                Ok(stream)
            })),
            Err(err) => Err(err),
        }
    }

    /// Dial a remote peer as a listener. This function is changed to perform an authentication
    /// handshake on top. The flow should be the same as the `dial` function.
    fn dial_as_listener(
        &mut self,
        addr: libp2p::Multiaddr,
    ) -> Result<Self::Dial, libp2p::TransportError<Self::Error>> {
        // Perform the inner dial
        let res = self.inner.dial(addr);

        // Clone the necessary fields
        let auth_message = Arc::clone(&self.auth_message);
        let stake_table = Arc::clone(&self.stake_table);

        // If the dial was successful, perform the authentication handshake on top
        match res {
            Ok(dial) => Ok(Box::pin(async move {
                // Perform the inner dial
                let mut stream = dial.await?;

                // Time out the authentication block
                async_timeout(AUTH_HANDSHAKE_TIMEOUT, async {
                    // Open a substream for the handshake
                    let mut substream =
                        poll_fn(|cx| stream.as_connection().poll_outbound_unpin(cx)).await?;

                    // (inbound) Verify the remote peer's authentication
                    Self::verify_peer_authentication(&mut substream, stake_table)
                        .await
                        .map_err(|e| {
                            warn!("Failed to verify remote peer: {:?}", e);
                            std::io::Error::new(std::io::ErrorKind::Other, e)
                        })?;

                    // (outbound) Authenticate with the remote peer
                    Self::authenticate_with_remote_peer(&mut substream, auth_message)
                        .await
                        .map_err(|e| {
                            warn!("Failed to authenticate with remote peer: {:?}", e);
                            std::io::Error::new(std::io::ErrorKind::Other, e)
                        })?;

                    Ok::<(), T::Error>(())
                })
                .await
                .map_err(|e| {
                    warn!("Timed out performing authentication handshake: {:?}", e);
                    std::io::Error::new(std::io::ErrorKind::TimedOut, e)
                })??;

                Ok(stream)
            })),
            Err(err) => Err(err),
        }
    }

    /// This function is where we perform the authentication handshake for _incoming_ connections.
    /// The flow in this case is the reverse of the `dial` function: we first verify the remote peer's
    /// authentication, and then authenticate with them.
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<libp2p::core::transport::TransportEvent<Self::ListenerUpgrade, Self::Error>>
    {
        match self.as_mut().project().inner.poll(cx) {
            Poll::Ready(event) => Poll::Ready(match event {
                // If we have an incoming connection, we need to perform the authentication handshake
                TransportEvent::Incoming {
                    listener_id,
                    upgrade,
                    local_addr,
                    send_back_addr,
                } => {
                    // Clone the necessary fields
                    let auth_message = Arc::clone(&self.auth_message);
                    let stake_table = Arc::clone(&self.stake_table);

                    // Create a new upgrade that performs the authentication handshake on top
                    let auth_upgrade = Box::pin(async move {
                        // Perform the inner upgrade
                        let mut stream = upgrade.await?;

                        // Time out the authentication block
                        async_timeout(AUTH_HANDSHAKE_TIMEOUT, async {
                            // Open a substream for the handshake
                            let mut substream =
                                poll_fn(|cx| stream.as_connection().poll_inbound_unpin(cx)).await?;

                            // (inbound) Verify the remote peer's authentication
                            Self::verify_peer_authentication(&mut substream, stake_table)
                                .await
                                .map_err(|e| {
                                    warn!("Failed to verify remote peer: {:?}", e);
                                    std::io::Error::new(std::io::ErrorKind::Other, e)
                                })?;

                            // (outbound) Authenticate with the remote peer
                            Self::authenticate_with_remote_peer(&mut substream, auth_message)
                                .await
                                .map_err(|e| {
                                    warn!("Failed to authenticate with remote peer: {:?}", e);
                                    std::io::Error::new(std::io::ErrorKind::Other, e)
                                })?;

                            Ok::<(), T::Error>(())
                        })
                        .await
                        .map_err(|e| {
                            warn!("Timed out performing authentication handshake: {:?}", e);
                            std::io::Error::new(std::io::ErrorKind::TimedOut, e)
                        })??;

                        Ok(stream)
                    });

                    // Return the new event
                    TransportEvent::Incoming {
                        listener_id,
                        upgrade: auth_upgrade,
                        local_addr,
                        send_back_addr,
                    }
                }

                // We need to re-map the other events because we changed the type of the upgrade
                TransportEvent::AddressExpired {
                    listener_id,
                    listen_addr,
                } => TransportEvent::AddressExpired {
                    listener_id,
                    listen_addr,
                },
                TransportEvent::ListenerClosed {
                    listener_id,
                    reason,
                } => TransportEvent::ListenerClosed {
                    listener_id,
                    reason,
                },
                TransportEvent::ListenerError { listener_id, error } => {
                    TransportEvent::ListenerError { listener_id, error }
                }
                TransportEvent::NewAddress {
                    listener_id,
                    listen_addr,
                } => TransportEvent::NewAddress {
                    listener_id,
                    listen_addr,
                },
            }),

            Poll::Pending => Poll::Pending,
        }
    }

    /// The below functions just pass through to the inner transport, but we had
    /// to define them
    fn remove_listener(&mut self, id: libp2p::core::transport::ListenerId) -> bool {
        self.inner.remove_listener(id)
    }
    fn address_translation(
        &self,
        listen: &libp2p::Multiaddr,
        observed: &libp2p::Multiaddr,
    ) -> Option<libp2p::Multiaddr> {
        self.inner.address_translation(listen, observed)
    }
    fn listen_on(
        &mut self,
        id: libp2p::core::transport::ListenerId,
        addr: libp2p::Multiaddr,
    ) -> Result<(), libp2p::TransportError<Self::Error>> {
        self.inner.listen_on(id, addr)
    }
}

/// A helper trait that allows us to access the underlying connection
trait AsConnection<C: StreamMuxer + Unpin> {
    /// Get a mutable reference to the underlying connection
    fn as_connection(&mut self) -> &mut C;
}

/// The implementation of the `AsConnection` trait for a tuple of a `PeerId`
/// and a connection.
impl<C: StreamMuxer + Unpin> AsConnection<C> for (PeerId, C) {
    fn as_connection(&mut self) -> &mut C {
        &mut self.1
    }
}

#[cfg(test)]
mod test {
    use hotshot_types::{signature_key::BLSPubKey, traits::signature_key::SignatureKey};

    /// Test valid construction and verification of an authentication message
    #[test]
    fn construct_and_verify_auth_message() {
        // Create a new keypair
        let keypair = BLSPubKey::generated_from_seed_indexed([1u8; 32], 1337);

        // Construct an authentication message
        let auth_message = super::construct_auth_message(&keypair.0, &keypair.1).unwrap();

        // Verify the authentication message
        let public_key = super::AuthMessage::<BLSPubKey>::validate(
            &bincode::deserialize(&auth_message).unwrap(),
        );
        assert!(public_key.is_ok());
    }

    /// Test invalid construction and verification of an authentication message
    #[test]
    fn construct_and_verify_invalid_auth_message() {
        // Create a new keypair
        let keypair = BLSPubKey::generated_from_seed_indexed([1u8; 32], 1337);

        // Construct an authentication message
        let auth_message = super::construct_auth_message(&keypair.0, &keypair.1).unwrap();

        // Change the public key in the message
        let mut auth_message: super::AuthMessage<BLSPubKey> =
            bincode::deserialize(&auth_message).unwrap();

        // Change the public key
        auth_message.pub_key_bytes[0] ^= 0x01;

        // Serialize the message again
        let auth_message = bincode::serialize(&auth_message).unwrap();

        // Verify the authentication message
        let public_key = super::AuthMessage::<BLSPubKey>::validate(
            &bincode::deserialize(&auth_message).unwrap(),
        );
        assert!(public_key.is_err());
    }

    #[cfg_attr(async_executor_impl = "tokio", tokio::test(flavor = "multi_thread"))]
    #[cfg_attr(async_executor_impl = "async-std", async_std::test)]
    async fn read_and_write_length_delimited() {
        // Create a message
        let message = b"Hello, world!";

        // Write the message to a buffer
        let mut buffer = Vec::new();
        super::write_length_delimited(&mut buffer, message)
            .await
            .unwrap();

        // Read the message from the buffer
        let read_message = super::read_length_delimited(&mut buffer.as_slice(), 1024)
            .await
            .unwrap();

        // Check if the messages are the same
        assert_eq!(message, read_message.as_slice());
    }
}
