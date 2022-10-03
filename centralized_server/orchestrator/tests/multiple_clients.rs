use commit::{Commitment, Committable};
use hotshot::{
    traits::{BlockContents, StateContents},
    types::SignatureKey,
};
use hotshot_centralized_server::{TcpStreamUtil, TcpStreamUtilWithRecv, TcpStreamUtilWithSend};
use hotshot_types::{
    data::ViewNumber,
    traits::signature_key::{EncodedPublicKey, EncodedSignature},
};
use hotshot_utils::{
    channel::oneshot,
    test_util::{setup_backtrace, setup_logging},
};
use std::{collections::HashSet, fmt, net::Ipv4Addr, time::Duration};
use tracing::instrument;

type Server = hotshot_centralized_server::Server<TestSignatureKey>;
type ToServer = hotshot_centralized_server::ToServer<TestSignatureKey>;
type FromServer = hotshot_centralized_server::FromServer<TestSignatureKey>;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
#[instrument]
async fn multiple_clients() {
    setup_logging();
    setup_backtrace();
    use hotshot_utils::art::{async_spawn, async_timeout};
    let (shutdown, shutdown_receiver) = oneshot();
    let server = Server::new(Ipv4Addr::LOCALHOST.into(), 0)
        .await
        .with_shutdown_signal(shutdown_receiver);
    let server_addr = server.addr();
    let server_join_handle = async_spawn(server.run());

    // Connect first client
    let first_client_key = TestSignatureKey { idx: 1 };
    let mut first_client = TcpStreamUtil::connect(server_addr).await.unwrap();
    first_client
        .send(ToServer::Identify {
            key: first_client_key.clone(),
        })
        .await
        .unwrap();

    // Assert that there is 1 client connected
    first_client
        .send(ToServer::RequestClientCount)
        .await
        .unwrap();
    let msg = first_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::ClientCount(1) => {}
        x => panic!("Expected ClientCount(1), got {x:?}"),
    }

    // Connect second client
    let second_client_key = TestSignatureKey { idx: 2 };
    let mut second_client = TcpStreamUtil::connect(server_addr).await.unwrap();
    second_client
        .send(ToServer::Identify {
            key: second_client_key.clone(),
        })
        .await
        .unwrap();

    // Assert that the first client gets a notification of this
    let msg = first_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::NodeConnected {
            key: TestSignatureKey { idx: 2 },
            ..
        } => {}
        x => panic!("Expected NodeConnected, got {x:?}"),
    }
    // Assert that there are 2 clients connected
    first_client
        .send(ToServer::RequestClientCount)
        .await
        .unwrap();
    let msg = first_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::ClientCount(2) => {}
        x => panic!("Expected ClientCount(2), got {x:?}"),
    }
    second_client
        .send(ToServer::RequestClientCount)
        .await
        .unwrap();
    let msg = second_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::ClientCount(2) => {}
        x => panic!("Expected ClientCount(2), got {x:?}"),
    }

    // Send a direct message from 1 -> 2
    let direct_message = vec![1, 2, 3, 4];
    let direct_message_len = direct_message.len();
    first_client
        .send(ToServer::Direct {
            target: TestSignatureKey { idx: 2 },
            message_len: direct_message_len as u64,
        })
        .await
        .unwrap();
    first_client
        .send_raw(&direct_message, direct_message_len)
        .await
        .unwrap();

    // Check that 2 received this
    let msg = second_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::Direct {
            source,
            payload_len,
            ..
        } => {
            assert_eq!(source, first_client_key);
            assert_eq!(payload_len, 0);
        }
        x => panic!("Expected Direct, got {:?}", x),
    }
    let payload_msg = second_client.recv::<FromServer>().await.unwrap();
    match payload_msg {
        FromServer::DirectPayload { payload_len, .. } => {
            assert_eq!(payload_len, direct_message_len as u64)
        }
        x => panic!("Expected DirectPayload, got {:?}", x),
    }
    if let Some(payload_len) = payload_msg.payload_len() {
        let payload = second_client.recv_raw(payload_len.into()).await.unwrap();
        assert!(payload.len() == direct_message_len);
        assert_eq!(payload, direct_message);
    } else {
        panic!("Expected payload");
    }

    let broadcast_message = vec![50, 40, 30, 20, 10];
    let broadcast_message_len = broadcast_message.len();
    // Send a broadcast from 2
    second_client
        .send(ToServer::Broadcast {
            message_len: broadcast_message_len as u64,
        })
        .await
        .unwrap();
    second_client
        .send_raw(&broadcast_message, broadcast_message_len)
        .await
        .unwrap();

    // Check that 1 received this
    let msg = first_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::Broadcast {
            source,
            payload_len,
            ..
        } => {
            assert_eq!(source, second_client_key);
            assert_eq!(payload_len, 0);
        }
        x => panic!("Expected Broadcast, got {:?}", x),
    }
    let payload_msg = first_client.recv::<FromServer>().await.unwrap();
    match &payload_msg {
        FromServer::BroadcastPayload {
            source,
            payload_len,
        } => {
            assert_eq!(source, &second_client_key);
            assert_eq!(*payload_len as usize, broadcast_message_len);
        }
        x => panic!("Expected BroadcastPayload, got {:?}", x),
    }
    if let Some(payload_len) = &payload_msg.payload_len() {
        let payload = first_client.recv_raw((*payload_len).into()).await.unwrap();
        assert!(payload.len() == broadcast_message_len);
        assert_eq!(payload, broadcast_message);
    } else {
        panic!("Expected payload");
    }

    // Disconnect the second client
    drop(second_client);

    // Assert that the first client received a notification of the second client disconnecting
    let msg = first_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::NodeDisconnected {
            key: TestSignatureKey { idx: 2 },
        } => {}
        x => panic!("Expected NodeDisconnected, got {x:?}"),
    }

    // Assert that the server reports 1 client being connected
    first_client
        .send(ToServer::RequestClientCount)
        .await
        .unwrap();
    let msg = first_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::ClientCount(1) => {}
        x => panic!("Expected ClientCount(1), got {x:?}"),
    }

    // Shut down the server
    shutdown.send(());

    let f = async_timeout(Duration::from_secs(5), server_join_handle).await;

    #[cfg(feature = "async-std-executor")]
    f.unwrap();

    #[cfg(feature = "tokio-executor")]
    f.unwrap().unwrap();
}

#[derive(Debug)]
enum TestError {}

impl fmt::Display for TestError {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{:?}", self)
    }
}
impl std::error::Error for TestError {}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq)]
struct TestBlock {}

impl Committable for TestBlock {
    fn commit(&self) -> Commitment<Self> {
        commit::RawCommitmentBuilder::new("Test Block Comm")
            .u64_field("Nothing", 0)
            .finalize()
    }
}

impl BlockContents for TestBlock {
    type Error = TestError;
    type Transaction = TestTransaction;

    fn add_transaction_raw(
        &self,
        _tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(Self {})
    }

    fn contained_transactions(&self) -> HashSet<Commitment<Self::Transaction>> {
        HashSet::default()
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq)]
struct TestTransaction {}

impl Committable for TestTransaction {
    fn commit(&self) -> Commitment<Self> {
        commit::RawCommitmentBuilder::new("Test Txn Comm")
            .u64_field("Nothing", 0)
            .finalize()
    }
}

#[derive(Clone, Default, serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq)]
struct TestState {}
impl Committable for TestState {
    fn commit(&self) -> Commitment<Self> {
        commit::RawCommitmentBuilder::new("Test Txn Comm")
            .u64_field("Nothing", 0)
            .finalize()
    }
}

impl StateContents for TestState {
    type Error = TestError;
    type Block = TestBlock;
    type Time = ViewNumber;

    fn next_block(&self) -> Self::Block {
        TestBlock {}
    }

    fn validate_block(&self, _block: &Self::Block, _time: &Self::Time) -> bool {
        true
    }

    fn append(&self, _block: &Self::Block, _time: &Self::Time) -> Result<Self, Self::Error> {
        Ok(Self {})
    }

    fn on_commit(&self) {}
}

#[derive(
    Clone, serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq, PartialOrd, Ord,
)]
struct TestSignatureKey {
    idx: u64,
}
impl SignatureKey for TestSignatureKey {
    type PrivateKey = u64;

    fn validate(
        &self,
        _signature: &hotshot_types::traits::signature_key::EncodedSignature,
        _data: &[u8],
    ) -> bool {
        true
    }

    fn sign(_private_key: &Self::PrivateKey, _data: &[u8]) -> EncodedSignature {
        EncodedSignature(vec![0u8; 16])
    }

    fn from_private(private_key: &Self::PrivateKey) -> Self {
        Self { idx: *private_key }
    }

    fn to_bytes(&self) -> EncodedPublicKey {
        EncodedPublicKey(self.idx.to_le_bytes().to_vec())
    }

    fn from_bytes(pubkey: &EncodedPublicKey) -> Option<Self> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&pubkey.0);
        Some(Self {
            idx: u64::from_le_bytes(bytes),
        })
    }
}
