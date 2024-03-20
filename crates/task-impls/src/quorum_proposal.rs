use std::sync::Arc;

use async_broadcast::Sender;
use hotshot_task::dependency_task::HandleDepOutput;
use hotshot_types::traits::node_implementation::NodeType;

use crate::events::HotShotEvent;

/// Proposal dependency types. These types represent events that precipitate a proposal.
#[derive(PartialEq)]
enum ProposalDependency {
    /// For the `SendPayloadCommitmentAndMetadata` event.
    SendPayloadCommitmentAndMetadata,

    /// For the `ViewSyncFinalizeCertificate2Recv` event.
    ViewSyncFinalizeCertificate2Recv,

    /// For the `QuorumProposalRecv` event.
    QuorumProposalRecv,

    /// For the `QCFormed` event.
    QCFormed,
}

struct ProposalDependencyHandle<TYPES: NodeType> {
    /// The view number to propose for.
    view_number: TYPES::Time,

    /// The event sender.
    sender: Sender<Arc<HotShotEvent<TYPES>>>,
}

impl<TYPES: NodeType> HandleDepOutput for ProposalDependencyHandle<TYPES> {
    type Output = Vec<Arc<HotShotEvent<TYPES>>>;

    async fn handle_dep_result(self, res: Self::Output) {
        todo!()
    }
}
