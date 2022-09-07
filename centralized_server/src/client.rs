use crate::{ClientConfig, Error, FromServer, Run, TcpStreamUtil, ToBackground, ToServer};
use async_std::net::TcpStream;
use flume::Sender;
use hotshot_types::traits::signature_key::SignatureKey;
use std::net::SocketAddr;
use tracing::debug;

pub(crate) async fn spawn<K: SignatureKey + 'static>(
    addr: SocketAddr,
    stream: TcpStream,
    sender: Sender<ToBackground<K>>,
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
            .send_async(ToBackground::ClientDisconnected { run, key })
            .await;
    }
}

async fn run_client<K: SignatureKey + 'static>(
    address: SocketAddr,
    stream: TcpStream,
    parent_key: &mut Option<K>,
    parent_run: &mut Option<Run>,
    to_background: Sender<ToBackground<K>>,
) -> Result<(), Error> {
    let (sender, receiver) = flume::unbounded();
    // Start up a loopback task, which will receive messages from the background (see `background_task` in `src/lib.rs`) and forward them to our `TcpStream`.
    async_std::task::spawn({
        let mut stream = TcpStreamUtil::new(stream.clone());
        async move {
            while let Ok(msg) = receiver.recv_async().await {
                if let Err(e) = stream.send(msg).await {
                    debug!("Lost connection to {:?}: {:?}", address, e);
                    break;
                }
            }
        }
    });

    // Get the network config and the run # from the background thread.
    let ClientConfig { run, config } = {
        let (sender, receiver) = flume::bounded(1);
        let _ = to_background
            .send_async(ToBackground::ClientConnected(sender))
            .await;
        receiver
            .recv_async()
            .await
            .expect("Could not get client info from background")
    };
    // Make sure to let `spawn` know what run # we have gotten
    *parent_run = Some(run);
    let mut stream = TcpStreamUtil::new(stream);
    loop {
        let msg = stream.recv::<ToServer<K>>().await?;

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
                    .send_async(ToBackground::NewClient { run, key, sender })
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
                    .send_async(FromServer::Config {
                        config: config.clone(),
                        run,
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            // The client requested the libp2p config
            (ToServer::GetLibp2pConfig, _) => {
                todo!()
            }
            // This will make sure that the cases below do not get called when we are not identified yet
            (_, false) => {
                debug!("{:?} received message but is not identified yet", address);
            }
            // Client wants to broadcast a message
            (ToServer::Broadcast { message }, true) => {
                let sender = parent_key.clone().unwrap();
                to_background
                    .send_async(ToBackground::IncomingBroadcast {
                        run,
                        sender,
                        message,
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            // Client wants to send a direct message to another client
            (ToServer::Direct { message, target }, true) => {
                to_background
                    .send_async(ToBackground::IncomingDirectMessage {
                        run,
                        receiver: target,
                        message,
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            // Client wants to know how many clients are connected in the current run
            (ToServer::RequestClientCount, true) => {
                to_background
                    .send_async(ToBackground::RequestClientCount {
                        run,
                        sender: parent_key.clone().unwrap(),
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            // Client wants to submit the results of this run
            (ToServer::Results(results), true) => {
                to_background
                    .send_async(ToBackground::Results { results })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
        }
    }
}
