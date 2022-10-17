use crate::{
    config::ClientConfig, Error, FromBackground, Run, TcpStreamRecvUtil, TcpStreamSendUtil,
    TcpStreamUtilWithRecv, TcpStreamUtilWithSend, ToBackground, ToServer,
};
use hotshot_types::traits::{signature_key::SignatureKey, election::ElectionConfig};
use hotshot_utils::{
    art::{async_spawn, split_stream},
    channel::{bounded, oneshot, Sender},
};
use std::{net::SocketAddr, num::NonZeroUsize};
use tracing::{debug, warn};

cfg_if::cfg_if! {
    if #[cfg(feature = "async-std-executor")] {
        use async_std::net::TcpStream;
    } else if #[cfg(feature = "tokio-executor")] {
        use tokio::net::TcpStream;
    } else {
        std::compile_error!{"Either feature \"async-std-executor\" or feature \"tokio-executor\" must be enabled for this crate."}
    }
}

pub(crate) async fn spawn<K: SignatureKey + 'static, E: ElectionConfig + 'static>(
    addr: SocketAddr,
    stream: TcpStream,
    sender: Sender<ToBackground<K, E>>,
) {
    // We want to know the signature key and the run # that this client ran on
    // so we store those here and pass a mutable reference to `run_client`
    // This way even when `run_client` encounters an error, we can properly disconnect from the network
    let mut key = None;
    let mut run = None;
    if let Err(e) = run_client(addr, stream, &mut key, &mut run, sender.clone()).await {
        debug!("Client from {:?} encountered an error: {:?}", addr, e);
    } else {
        debug!("Client from {:?} shut down", addr);
    }
    // if we were far enough into `run_client` to obtain a signature key and run #, properly disconnect from the network
    if let (Some(key), Some(run)) = (key, run) {
        let _ = sender
            .send(ToBackground::ClientDisconnected { run, key })
            .await;
    }
}

async fn run_client<K: SignatureKey + 'static, E: ElectionConfig + 'static>(
    address: SocketAddr,
    stream: TcpStream,
    parent_key: &mut Option<K>,
    parent_run: &mut Option<Run>,
    to_background: Sender<ToBackground<K, E>>,
) -> Result<(), Error>
{
    let (read_stream, write_stream) = split_stream(stream);

    let (sender, mut receiver) = bounded::<FromBackground<K, E>>(10);

    // Start up a loopback task, which will receive messages from the background (see `background_task` in `src/lib.rs`) and forward them to our `TcpStream`.
    async_spawn({
        let mut send_stream = TcpStreamSendUtil::new(write_stream);
        async move {
            while let Ok(msg) = receiver.recv().await {
                let FromBackground { header, payload } = msg;
                let payload_len = payload.as_ref().map(|p| p.len()).unwrap_or(0);
                if let Some(payload_expected_len) = header.payload_len() {
                    if payload_len != <NonZeroUsize as Into<usize>>::into(payload_expected_len) {
                        warn!(?header, "expecting {payload_expected_len} bytes, but payload has {payload_len} bytes");
                        break;
                    }
                } else if payload_len > 0 {
                    warn!(
                        ?header,
                        "expecting no payload, but payload has {payload_len} bytes"
                    );
                    break;
                }
                if let Err(e) = send_stream.send(header).await {
                    debug!("Lost connection to {:?}: {:?}", address, e);
                    break;
                }
                if let Some(payload) = payload {
                    if !payload.is_empty() {
                        if let Err(e) = send_stream.send_raw(&payload, payload.len()).await {
                            debug!("Lost connection to {:?}: {:?}", address, e);
                            break;
                        }
                    }
                }
            }
        }
    });

    // Get the network config and the run # from the background thread.
    let ClientConfig { run, config } = {
        let (sender, receiver) = oneshot();
        let _ = to_background
            .send(ToBackground::ClientConnected {
                addr: address,
                sender,
            })
            .await;
        receiver
            .recv()
            .await
            .expect("Could not get client info from background")
    };
    // Make sure to let `spawn` know what run # we have gotten
    *parent_run = Some(run);

    let mut recv_stream = TcpStreamRecvUtil::new(read_stream);
    loop {
        let msg = recv_stream.recv::<ToServer<K>>().await?;

        // Most of these messages are mapped to `ToBackground` and send to the background thread.
        // See `background_task` in `src/lib.rs` for more information
        match (msg, parent_key.is_some()) {
            // Client tries to identify with the given signature key `key`, and we don't have key yet
            (ToServer::Identify { key }, false) => {
                // set the key for `spawn` so we can properly disconnect
                *parent_key = Some(key.clone());
                let sender = sender.clone();
                // register with the background
                to_background
                    .send(ToBackground::NewClient { run, key, sender })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            // Client tried to identify but was already identified
            (ToServer::Identify { .. }, true) => {
                debug!("{:?} tried to identify twice", address);
            }
            // The client requested the config
            (ToServer::GetConfig, _) => {
                sender
                    .send(FromBackground::config(config.clone(), run))
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            // This will make sure that the cases below do not get called when we are not identified yet
            (_, false) => {
                debug!("{:?} received message but is not identified yet", address);
            }
            // Client wants to broadcast a message
            (ToServer::Broadcast { message_len }, true) => {
                let sender = parent_key.clone().unwrap();
                to_background
                    .send(ToBackground::IncomingBroadcast {
                        run,
                        sender: sender.clone(),
                        message_len,
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
                let mut remaining = message_len as usize;

                while remaining > 0 {
                    let message_chunk = recv_stream
                        .recv_raw(remaining.min(crate::MAX_CHUNK_SIZE))
                        .await?;
                    remaining -= message_chunk.len();
                    to_background
                        .send(ToBackground::IncomingBroadcastChunk {
                            run,
                            sender: sender.clone(),
                            message_chunk,
                        })
                        .await
                        .map_err(|_| Error::BackgroundShutdown)?;
                }
            }
            // Client wants to send a direct message to another client
            (
                ToServer::Direct {
                    message_len,
                    target,
                },
                true,
            ) => {
                let sender = parent_key.clone().unwrap();
                to_background
                    .send(ToBackground::IncomingDirectMessage {
                        run,
                        sender: sender.clone(),
                        receiver: target.clone(),
                        message_len,
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
                let mut remaining = message_len as usize;
                while remaining > 0 {
                    let message_chunk = recv_stream
                        .recv_raw(remaining.min(crate::MAX_CHUNK_SIZE))
                        .await?;
                    remaining -= message_chunk.len();
                    to_background
                        .send(ToBackground::IncomingDirectMessageChunk {
                            run,
                            sender: sender.clone(),
                            receiver: target.clone(),
                            message_chunk,
                        })
                        .await
                        .map_err(|_| Error::BackgroundShutdown)?;
                }
            }
            // Client wants to know how many clients are connected in the current run
            (ToServer::RequestClientCount, true) => {
                to_background
                    .send(ToBackground::RequestClientCount {
                        run,
                        sender: parent_key.clone().unwrap(),
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            // Client wants to submit the results of this run
            (ToServer::Results(results), true) => {
                to_background
                    .send(ToBackground::Results { results })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
        }
    }
}
