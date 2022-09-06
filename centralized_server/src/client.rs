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
    let mut key = None;
    let mut run = None;
    if let Err(e) = run_client(addr, stream, &mut key, &mut run, sender.clone()).await {
        debug!("Client from {:?} encountered an error: {:?}", addr, e);
    } else {
        debug!("Client from {:?} shut down", addr);
    }
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
    *parent_run = Some(run);
    let mut stream = TcpStreamUtil::new(stream);
    loop {
        let msg = stream.recv::<ToServer<K>>().await?;
        let parent_key_is_some = parent_key.is_some();
        match (msg, parent_key_is_some) {
            (ToServer::Identify { key }, false) => {
                *parent_key = Some(key.clone());
                let sender = sender.clone();
                to_background
                    .send_async(ToBackground::NewClient { run, key, sender })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (ToServer::Identify { .. }, true) => {
                debug!("{:?} tried to identify twice", address);
            }
            (ToServer::GetConfig, _) => {
                sender
                    .send_async(FromServer::Config {
                        config: config.clone(),
                        run,
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (_, false) => {
                debug!("{:?} received message but is not identified yet", address);
            }
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
            (ToServer::RequestClientCount, true) => {
                to_background
                    .send_async(ToBackground::RequestClientCount {
                        run,
                        sender: parent_key.clone().unwrap(),
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (ToServer::Results(results), true) => {
                to_background
                    .send_async(ToBackground::Results { results })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
        }
    }
}
