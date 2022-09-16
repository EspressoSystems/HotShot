use commit::{Commitment, Committable};
use hotshot::{
    traits::{BlockContents, StateContents},
    types::SignatureKey,
};
use hotshot_centralized_server::TcpStreamUtil;
use hotshot_types::traits::signature_key::{EncodedPublicKey, EncodedSignature};
use std::{collections::HashSet, fmt, net::Ipv4Addr, time::Duration};

type Server = hotshot_centralized_server::Server<TestSignatureKey>;
type ToServer = hotshot_centralized_server::ToServer<TestSignatureKey>;
type FromServer = hotshot_centralized_server::FromServer<TestSignatureKey>;

#[cfg_attr(
    feature = "tokio-executor",
    tokio::test(flavor = "multi_thread", worker_threads = 2)
)]
#[cfg_attr(feature = "async-std-executor", async_std::test)]
async fn multiple_clients() {
    use hotshot_utils::art::{async_spawn, async_timeout};
    let (shutdown, shutdown_receiver) = flume::bounded(1);
    let server = Server::new(Ipv4Addr::LOCALHOST.into(), 0)
        .await
        .with_shutdown_signal(shutdown_receiver);
    let server_addr = server.addr();
    let server_join_handle = async_spawn(server.run());

    // Connect first client
    let mut first_client = TcpStreamUtil::connect(server_addr).await.unwrap();
    first_client
        .send(ToServer::Identify {
            key: TestSignatureKey { idx: 1 },
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
    let mut second_client = TcpStreamUtil::connect(server_addr).await.unwrap();
    second_client
        .send(ToServer::Identify {
            key: TestSignatureKey { idx: 2 },
        })
        .await
        .unwrap();

    // Assert that the first client gets a notification of this
    let msg = first_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::NodeConnected {
            key: TestSignatureKey { idx: 2 },
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
    let message = vec![1, 2, 3, 4];
    first_client
        .send(ToServer::Direct {
            target: TestSignatureKey { idx: 2 },
            message_len: (message.len() * std::mem::size_of::<i32>()) as u64,
        })
        .await
        .unwrap();
    first_client
        .send_raw(&message, message.len() * std::mem::size_of::<i32>())
        .await
        .unwrap();

    // Check that 2 received this
    let msg = second_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::Direct { message } => assert_eq!(message, vec![1, 2, 3, 4]),
        x => panic!("Expected Direct, got {:?}", x),
    }

    // Send a broadcast from 2
    second_client
        .send(ToServer::Broadcast {
            message: vec![50, 40, 30, 20, 10],
        })
        .await
        .unwrap();

    // Check that 1 received this
    let msg = first_client.recv::<FromServer>().await.unwrap();
    match msg {
        FromServer::Broadcast { message } => assert_eq!(message, vec![50, 40, 30, 20, 10]),
        x => panic!("Expected Broadcast, got {:?}", x),
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
    shutdown.send(()).unwrap();
    async_timeout(Duration::from_secs(5), server_join_handle)
        .await
        .expect("Could not shut down server");
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

    fn next_block(&self) -> Self::Block {
        TestBlock {}
    }

    fn validate_block(&self, _block: &Self::Block) -> bool {
        true
    }

    fn append(&self, _block: &Self::Block) -> Result<Self, Self::Error> {
        Ok(Self {})
    }

    fn on_commit(&self) {}
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq)]
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
