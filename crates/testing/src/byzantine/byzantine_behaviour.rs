use async_trait::async_trait;
use hotshot::tasks::EventTransformerState;
use hotshot_task_impls::events::HotShotEvent;
use hotshot_types::{
    data::QuorumProposal,
    message::Proposal,
    traits::node_implementation::{NodeImplementation, NodeType, Versions},
};
use std::collections::{hash_map::Entry, HashMap, HashSet};

#[derive(Debug)]
/// An `EventTransformerState` that multiplies `QuorumProposalSend` events, incrementing the view number of the proposal
pub struct BadProposalViewDos {
    /// The number of times to duplicate a `QuorumProposalSend` event
    pub multiplier: u64,
    /// The view number increment each time it's duplicatedjust
    pub increment: u64,
}

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> EventTransformerState<TYPES, I, V>
    for BadProposalViewDos
{
    async fn recv_handler(&mut self, event: &HotShotEvent<TYPES>) -> Vec<HotShotEvent<TYPES>> {
        vec![event.clone()]
    }

    async fn send_handler(&mut self, event: &HotShotEvent<TYPES>) -> Vec<HotShotEvent<TYPES>> {
        match event {
            HotShotEvent::QuorumProposalSend(proposal, signature) => {
                let mut result = Vec::new();

                for n in 0..self.multiplier {
                    let mut modified_proposal = proposal.clone();

                    modified_proposal.data.view_number += n * self.increment;

                    result.push(HotShotEvent::QuorumProposalSend(
                        modified_proposal,
                        signature.clone(),
                    ));
                }

                result
            }
            _ => vec![event.clone()],
        }
    }
}

#[derive(Debug)]
/// An `EventHandlerState` that doubles the `QuorumVoteSend` and `QuorumProposalSend` events
pub struct DoubleProposeVote;

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES>, V: Versions> EventTransformerState<TYPES, I, V>
    for DoubleProposeVote
{
    async fn recv_handler(&mut self, event: &HotShotEvent<TYPES>) -> Vec<HotShotEvent<TYPES>> {
        vec![event.clone()]
    }

    async fn send_handler(&mut self, event: &HotShotEvent<TYPES>) -> Vec<HotShotEvent<TYPES>> {
        match event {
            HotShotEvent::QuorumProposalSend(_, _) | HotShotEvent::QuorumVoteSend(_) => {
                vec![event.clone(), event.clone()]
            }
            _ => vec![event.clone()],
        }
    }
}

#[derive(Debug)]
/// An `EventHandlerState` that modifies justify_qc on `QuorumProposalSend` to that of a previous view to mock dishonest leader
pub struct DishonestLeader<TYPES: NodeType> {
    /// Store events from previous views
    pub validated_proposals: Vec<QuorumProposal<TYPES>>,
    /// How many times current node has been elected leader and sent proposal
    pub total_proposals_from_node: u64,
    /// Which proposals to be dishonest at
    pub dishonest_at_proposal_numbers: HashSet<u64>,
    /// How far back to look for a QC
    pub view_look_back: usize,
}

/// Add method that will handle `QuorumProposalSend` events
/// If we have previous proposals stored and the total_proposals_from_node matches a value specified in dishonest_at_proposal_numbers
/// Then send out the event with the modified proposal that has an older QC
impl<TYPES: NodeType> DishonestLeader<TYPES> {
    /// When a leader is sending a proposal this method will mock a dishonest leader
    /// We accomplish this by looking back a number of specified views and using that cached proposals QC
    fn handle_proposal_send_event(
        &self,
        event: &HotShotEvent<TYPES>,
        proposal: &Proposal<TYPES, QuorumProposal<TYPES>>,
        sender: &TYPES::SignatureKey,
    ) -> HotShotEvent<TYPES> {
        let length = self.validated_proposals.len();
        if !self
            .dishonest_at_proposal_numbers
            .contains(&self.total_proposals_from_node)
            || length == 0
        {
            return event.clone();
        }

        // Grab proposal from specified view look back
        let proposal_from_look_back = if length - 1 < self.view_look_back {
            // If look back is too far just take the first proposal
            self.validated_proposals[0].clone()
        } else {
            let index = (self.validated_proposals.len() - 1) - self.view_look_back;
            self.validated_proposals[index].clone()
        };

        // Create a dishonest proposal by using the old proposals qc
        let mut dishonest_proposal = proposal.clone();
        dishonest_proposal.data.justify_qc = proposal_from_look_back.justify_qc;

        HotShotEvent::QuorumProposalSend(dishonest_proposal, sender.clone())
    }
}

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES> + std::fmt::Debug, V: Versions>
    EventTransformerState<TYPES, I, V> for DishonestLeader<TYPES>
{
    async fn recv_handler(&mut self, event: &HotShotEvent<TYPES>) -> Vec<HotShotEvent<TYPES>> {
        vec![event.clone()]
    }

    async fn send_handler(&mut self, event: &HotShotEvent<TYPES>) -> Vec<HotShotEvent<TYPES>> {
        match event {
            HotShotEvent::QuorumProposalSend(proposal, sender) => {
                self.total_proposals_from_node += 1;
                return vec![self.handle_proposal_send_event(event, proposal, sender)];
            }
            HotShotEvent::QuorumProposalValidated(proposal, _) => {
                self.validated_proposals.push(proposal.clone());
            }
            _ => {}
        }
        vec![event.clone()]
    }
}

#[derive(Debug)]
/// An `EventHandlerState` that modifies view number on the certificate of `DacSend` event to that of a future view
pub struct DishonestDa {
    /// How many times current node has been elected leader and sent Da Cert
    pub total_da_certs_sent_from_node: u64,
    /// Which proposals to be dishonest at
    pub dishonest_at_da_cert_sent_numbers: HashSet<u64>,
    /// When leader how many times we will send DacSend and increment view number
    pub total_views_add_to_cert: u64,
}

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES> + std::fmt::Debug, V: Versions>
    EventTransformerState<TYPES, I, V> for DishonestDa
{
    async fn recv_handler(&mut self, event: &HotShotEvent<TYPES>) -> Vec<HotShotEvent<TYPES>> {
        vec![event.clone()]
    }

    async fn send_handler(&mut self, event: &HotShotEvent<TYPES>) -> Vec<HotShotEvent<TYPES>> {
        if let HotShotEvent::DacSend(cert, sender) = event {
            self.total_da_certs_sent_from_node += 1;
            if self
                .dishonest_at_da_cert_sent_numbers
                .contains(&self.total_da_certs_sent_from_node)
            {
                let mut result = vec![HotShotEvent::DacSend(cert.clone(), sender.clone())];
                for i in 1..=self.total_views_add_to_cert {
                    let mut bad_cert = cert.clone();
                    bad_cert.view_number = cert.view_number + i;
                    result.push(HotShotEvent::DacSend(bad_cert, sender.clone()));
                }
                return result;
            }
        }
        vec![event.clone()]
    }
}

#[derive(Debug)]
pub struct ViewDelay<TYPES: NodeType> {
    pub number_of_views_to_delay: u64,
    pub rcv_events_to_last_seen_view: HashMap<String, (u64, HotShotEvent<TYPES>)>,
    pub node_id: u64,
}

impl<TYPES: NodeType> ViewDelay<TYPES> {
    fn extract_view_number(&self, event_name: &str) -> Option<(String, u64)> {
        // let full_name = &event.to_string();
        let (sanitized_name, view_num) = event_name.split_once('(').unwrap_or((event_name, ""));
        if view_num.starts_with("view_number=ViewNumber(") {
            if let Some(start) = view_num.find('(') {
                if let Some(end) = view_num.find(')') {
                    let number_str = &view_num[start + 1..end];
                    let view_num = number_str.parse().unwrap();
                    return Some((sanitized_name.to_string(), view_num));
                }
            }
        }
        None
    }
}

#[async_trait]
impl<TYPES: NodeType, I: NodeImplementation<TYPES> + std::fmt::Debug, V: Versions>
    EventTransformerState<TYPES, I, V> for ViewDelay<TYPES>
{
    async fn recv_handler(&mut self, event: &HotShotEvent<TYPES>) -> Vec<HotShotEvent<TYPES>> {
        match self.extract_view_number(&event.to_string()) {
            Some(parsed_event) => {
                match self
                    .rcv_events_to_last_seen_view
                    .entry(parsed_event.0.to_string())
                {
                    Entry::Occupied(mut entry) => {
                        // tracing::error!("occupied: {:?} {:?}", entry.key(), entry.get());
                        let value = entry.get_mut();
                        let old = value.clone();
                        tracing::error!(
                            "event: {}, {} - {}",
                            parsed_event.0,
                            parsed_event.1,
                            value.0
                        );
                        if parsed_event.1 - value.0 > self.number_of_views_to_delay {
                            *value = (parsed_event.1, event.clone());
                        }
                        // tracing::error!("event: {:?} should be view {:?} but sending view {:?}", parsed_event.0, parsed_event.1, old.0);
                        return vec![old.1.clone()];
                    }
                    Entry::Vacant(vacant) => {
                        tracing::error!("vacant: {:?}", vacant.key());
                        vacant.insert((parsed_event.1, event.clone()));
                    }
                }
            }
            None => {
                tracing::error!("None");
            }
        }
        vec![event.clone()]
    }

    async fn send_handler(&mut self, event: &HotShotEvent<TYPES>) -> Vec<HotShotEvent<TYPES>> {
        vec![event.clone()]
    }
}
