use crate::network::behaviours::direct_message::DMEvent;
use libp2p::{
    core::Endpoint,
    request_response::Event,
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, ListenOpts, ListenerClosed, ListenerError,
        NetworkBehaviour, NetworkBehaviour, NewListenAddr, Swarm, Swarm, SwarmEvent, SwarmEvent,
        THandler, THandlerInEvent, THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
};
use std::collections::HashSet;
use std::iter;

use libp2p_swarm_test::SwarmExt;

#[async_std::test]
async fn direct_messenger() {
    let mut swarm1 = new_ephemeral(|_| Behaviour::default());
    let mut swarm2 = new_ephemeral(|_| Behaviour::default());

    let (swarm2_mem_listen_addr, _) = swarm2.listen().with_memory_addr_external().await;
    let swarm2_peer_id = *swarm2.local_peer_id();
    swarm1.connect(&mut swarm2).await;

    let swarm_events = futures::stream::poll_fn(|cx| swarm1.poll_next_unpin(cx))
        .take(8)
        .collect::<Vec<_>>()
        .await;

    let infos = swarm_events
        .iter()
        .filter_map(|e| match e {
            SwarmEvent::Behaviour(identify::Event::Received { info, .. }) => Some(info.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    for addrs in listen_addrs {
        assert_eq!(addrs.len(), 1);
        assert!(addrs.contains(&swarm2_mem_listen_addr));
    }
    //#TODO: After smoke test we should test request-response direct message retries
}

#[derive(Default)]
struct Behaviour {
    in_progress_rr: HashMap<OutboundRequestId, DMRequest>,
    out_event_queue: Vec<DMEvent>,
}

impl Behaviour {
    pub(crate) fn send_message(&mut self, event: Event<Vec<u8>, Vec<u8>>) {
        match event {
            Event::Message { message, peer, .. } => match message {
                Message::Request {
                    request: msg,
                    channel,
                    ..
                } => {
                    assert!(self
                        .out_event_queue
                        .contains(DMEvent::DirectRequest(msg, peer, channel)))
                }
                Message::Response {
                    request_id,
                    response: msg,
                } => {
                    assert!(!self.in_progress_rr.contains(&request_id));
                    assert!(self
                        .out_event_queue
                        .contains(DMEvent::DirectResponse(msg, request_id.peer_id)))
                }
            },
            //#TODO: Write a test to ensure that  Outbound failures do exponential backoff
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = dummy::ConnectionHandler;

    fn poll(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<ToSwarm<DMEvent, THandlerInEvent<Self>>> {
        Poll::Pending
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::derive_prelude::FromSwarm<'_>) {}

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
    }

    fn handle_established_inbound_connection(
        &mut self,
        _: ConnectionId,
        _: PeerId,
        _: &Multiaddr,
        _: &Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: libp2p::core::Endpoint,
    ) -> Result<libp2p::swarm::THandler<Self>, ConnectionDenied> {
        Ok(dummy::ConnectionHandler)
    }
}
