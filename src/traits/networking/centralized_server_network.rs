use async_trait::async_trait;
use hotshot_types::traits::{
    network::{NetworkingImplementation, TestableNetworkingImplementation},
    signature_key::{SignatureKey, TestableSignatureKey},
};
use serde::{de::DeserializeOwned, Serialize};

#[derive(Clone, Debug)]
pub struct CentralizedServerNetwork {}

#[async_trait]
impl<M, P> NetworkingImplementation<M, P> for CentralizedServerNetwork
where
    M: Serialize + DeserializeOwned + Send + Clone + 'static,
    P: SignatureKey + 'static,
{
    async fn ready(&self) -> bool {
        todo!()
    }

    async fn broadcast_message(
        &self,
        message: M,
    ) -> Result<(), hotshot_types::traits::network::NetworkError> {
        todo!()
    }

    async fn message_node(
        &self,
        message: M,
        recipient: P,
    ) -> Result<(), hotshot_types::traits::network::NetworkError> {
        todo!()
    }

    async fn broadcast_queue(
        &self,
    ) -> Result<Vec<M>, hotshot_types::traits::network::NetworkError> {
        todo!()
    }

    async fn next_broadcast(&self) -> Result<M, hotshot_types::traits::network::NetworkError> {
        todo!()
    }

    async fn direct_queue(&self) -> Result<Vec<M>, hotshot_types::traits::network::NetworkError> {
        todo!()
    }

    async fn next_direct(&self) -> Result<M, hotshot_types::traits::network::NetworkError> {
        todo!()
    }

    async fn known_nodes(&self) -> Vec<P> {
        todo!()
    }

    async fn network_changes(
        &self,
    ) -> Result<
        Vec<hotshot_types::traits::network::NetworkChange<P>>,
        hotshot_types::traits::network::NetworkError,
    > {
        todo!()
    }

    async fn shut_down(&self) -> () {
        todo!()
    }

    async fn put_record(
        &self,
        key: impl serde::Serialize + Send + Sync + 'static,
        value: impl serde::Serialize + Send + Sync + 'static,
    ) -> Result<(), hotshot_types::traits::network::NetworkError> {
        todo!()
    }

    async fn get_record<V: for<'a> serde::Deserialize<'a>>(
        &self,
        key: impl serde::Serialize + Send + Sync + 'static,
    ) -> Result<V, hotshot_types::traits::network::NetworkError> {
        todo!()
    }
}

impl<M, P> TestableNetworkingImplementation<M, P> for CentralizedServerNetwork
where
    M: Serialize + DeserializeOwned + Send + Clone + 'static,
    P: TestableSignatureKey + 'static,
{
    fn generator(
        expected_node_count: usize,
        num_bootstrap: usize,
    ) -> Box<dyn Fn(u64) -> Self + 'static> {
        todo!()
    }

    fn in_flight_message_count(&self) -> Option<usize> {
        todo!()
    }
}
