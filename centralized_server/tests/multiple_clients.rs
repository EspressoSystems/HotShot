use async_std::{
    io::{ReadExt, WriteExt},
    net::TcpStream,
    prelude::FutureExt,
};
use bincode::Options;
use hotshot::{
    data::{BlockHash, TransactionHash},
    traits::{BlockContents, State, Transaction},
    types::SignatureKey,
};
use hotshot_centralized_server_shared::MAX_MESSAGE_SIZE;
use hotshot_types::traits::signature_key::{EncodedPublicKey, EncodedSignature};
use std::{
    fmt,
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

type Server = hotshot_centralized_server::Server<TestSignatureKey>;
type ToServer = hotshot_centralized_server_shared::ToServer<TestSignatureKey>;

type FromServer = hotshot_centralized_server_shared::FromServer<TestSignatureKey>;

#[async_std::test]
async fn multiple_clients() {
    let (shutdown, shutdown_receiver) = flume::bounded(1);
    let server = Server::new(Ipv4Addr::LOCALHOST.into(), 0)
        .await
        .with_shutdown_signal(shutdown_receiver);
    let server_addr = server.addr();
    let server_join_handle = async_std::task::spawn(server.run());

    let mut first_client = TestClient::connect(server_addr).await.unwrap();
    first_client
        .send(ToServer::Identify {
            key: TestSignatureKey { idx: 1 },
        })
        .await
        .unwrap();

    let mut second_client = TestClient::connect(server_addr).await.unwrap();
    second_client
        .send(ToServer::Identify {
            key: TestSignatureKey { idx: 2 },
        })
        .await
        .unwrap();

    let msg = first_client.receive().await.unwrap();
    assert_eq!(
        msg,
        FromServer::NodeConnected {
            key: TestSignatureKey { idx: 2 }
        }
    );

    first_client
        .send(ToServer::Direct {
            target: TestSignatureKey { idx: 2 },
            message: vec![1, 2, 3, 4],
        })
        .await
        .unwrap();

    let msg = second_client.receive().await.unwrap();
    assert_eq!(
        msg,
        FromServer::Direct {
            message: vec![1, 2, 3, 4]
        }
    );

    second_client
        .send(ToServer::Broadcast {
            message: vec![50, 40, 30, 20, 10],
        })
        .await
        .unwrap();

    let msg = first_client.receive().await.unwrap();
    assert_eq!(
        msg,
        FromServer::Broadcast {
            message: vec![50, 40, 30, 20, 10],
        }
    );

    drop(second_client);
    let msg = first_client.receive().await.unwrap();
    assert_eq!(
        msg,
        FromServer::NodeDisconnected {
            key: TestSignatureKey { idx: 2 }
        }
    );
    shutdown.send(()).unwrap();
    server_join_handle
        .timeout(Duration::from_secs(5))
        .await
        .expect("Could not shut down server");
}

struct TestClient {
    stream: TcpStream,
}

impl TestClient {
    pub async fn connect(addr: SocketAddr) -> std::io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self { stream })
    }

    pub async fn send(&mut self, data: ToServer) -> std::io::Result<()> {
        let bytes = hotshot_centralized_server_shared::bincode_opts()
            .serialize(&data)
            .unwrap();
        if bytes.len() > MAX_MESSAGE_SIZE {
            eprintln!(
                "Send message is {} bytes but we only support {} bytes",
                bytes.len(),
                MAX_MESSAGE_SIZE
            );
            panic!("Please increase the value of `MAX_MESSAGE_SIZE`");
        }
        self.stream.write_all(&bytes).await
    }

    pub async fn receive(&mut self) -> std::io::Result<FromServer> {
        let mut buffer = [0u8; MAX_MESSAGE_SIZE];
        let n = self.stream.read(&mut buffer).await?;
        let result = hotshot_centralized_server_shared::bincode_opts()
            .deserialize(&buffer[..n])
            .expect("Invalid data received");
        Ok(result)
    }
}

impl Drop for TestClient {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.stream.shutdown(std::net::Shutdown::Both).unwrap();
        }
    }
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

impl<const N: usize> BlockContents<N> for TestBlock {
    type Error = TestError;
    type Transaction = TestTransaction;

    fn add_transaction_raw(
        &self,
        _tx: &Self::Transaction,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(Self {})
    }

    fn hash(&self) -> BlockHash<N> {
        BlockHash::default()
    }

    fn hash_transaction(_tx: &Self::Transaction) -> TransactionHash<N> {
        TransactionHash::default()
    }

    fn hash_leaf(bytes: &[u8]) -> hotshot::data::LeafHash<N> {
        assert_eq!(bytes.len(), N);
        let mut arr = [0u8; N];
        arr.copy_from_slice(bytes);
        arr.into()
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq)]
struct TestTransaction {}
impl<const N: usize> Transaction<N> for TestTransaction {}

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug, Hash, Eq, PartialEq)]
struct TestState {}
impl<const N: usize> State<N> for TestState {
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
