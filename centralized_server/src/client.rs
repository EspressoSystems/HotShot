use crate::{Error, FromServer, TcpStreamUtil, ToBackground, ToServer};
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
    if let Err(e) = run(addr, stream, &mut key, sender.clone()).await {
        debug!("Client from {:?} encountered an error: {:?}", addr, e);
    } else {
        debug!("Client from {:?} shut down", addr);
    }
    if let Some(key) = key {
        let _ = sender
            .send_async(ToBackground::ClientDisconnected { key })
            .await;
    }
}

async fn run<K: SignatureKey + 'static>(
    address: SocketAddr,
    stream: TcpStream,
    parent_key: &mut Option<K>,
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
    let mut stream = TcpStreamUtil::new(stream);
    loop {
        let msg = stream.recv::<ToServer<K>>().await?;
        let parent_key_is_some = parent_key.is_some();
        match (msg, parent_key_is_some) {
            (ToServer::Identify { key }, false) => {
                *parent_key = Some(key.clone());
                let sender = sender.clone();
                to_background
                    .send_async(ToBackground::NewClient { key, sender })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (ToServer::Identify { .. }, true) => {
                debug!("{:?} tried to identify twice", address);
            }
            (ToServer::GetConfig, _) => {
                let (config, run) = {
                    let (sender, receiver) = flume::bounded(1);
                    to_background
                        .send_async(ToBackground::RequestConfig { sender })
                        .await
                        .map_err(|_| Error::BackgroundShutdown)?;
                    receiver
                        .recv_async()
                        .await
                        .expect("Failed to retrieve config from background")
                };
                sender
                    .send_async(FromServer::Config { config, run })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (_, false) => {
                debug!("{:?} received message but is not identified yet", address);
            }
            (ToServer::Broadcast { message }, true) => {
                let sender = parent_key.clone().unwrap();
                to_background
                    .send_async(ToBackground::IncomingBroadcast { sender, message })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (ToServer::Direct { message, target }, true) => {
                to_background
                    .send_async(ToBackground::IncomingDirectMessage {
                        receiver: target,
                        message,
                    })
                    .await
                    .map_err(|_| Error::BackgroundShutdown)?;
            }
            (ToServer::RequestClientCount, true) => {
                to_background
                    .send_async(ToBackground::RequestClientCount {
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
