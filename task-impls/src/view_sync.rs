use crate::events::SequencingHotShotEvent;
use async_compatibility_layer::channel::UnboundedStream;
#[cfg(feature = "async-std-executor")]
use async_std::task::JoinHandle;
use either::Either::{self, Left, Right};
use futures::StreamExt;
use hotshot_task::task_impls::TaskBuilder;
use hotshot_task::{
    event_stream::{ChannelStream, EventStream},
    task::{FilterEvent, TaskErr, TS},
    task_impls::HSTWithEvent,
};
use hotshot_types::data::SequencingLeaf;
use hotshot_types::data::ViewNumber;
use hotshot_types::message::SequencingMessage;
use hotshot_types::message::ViewSyncMessageType;
use hotshot_types::traits::consensus_type::sequencing_consensus::SequencingConsensus;
use hotshot_types::traits::node_implementation::NodeImplementation;
use hotshot_types::traits::node_implementation::NodeType;
use snafu::Snafu;
use std::{marker::PhantomData, sync::Arc};
use tracing::{error, info, warn};

pub struct ViewSyncTaskState<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
> {
    pub event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    pub filtered_event_stream: UnboundedStream<SequencingHotShotEvent<TYPES, I>>,

    pub current_view: ViewNumber,
    pub next_view: ViewNumber,

    current_replica_task: Option<ViewNumber>,
    current_relay_task: Option<ViewNumber>,
}

pub struct ViewSyncReplicaTask<
    TYPES: NodeType<ConsensusType = SequencingConsensus>,
    I: NodeImplementation<
        TYPES,
        Leaf = SequencingLeaf<TYPES>,
        ConsensusMessage = SequencingMessage<TYPES, I>,
    >,
> {
    phantom: PhantomData<(TYPES, I)>
}

impl <
TYPES: NodeType<ConsensusType = SequencingConsensus>,
I: NodeImplementation<
    TYPES,
    Leaf = SequencingLeaf<TYPES>,
    ConsensusMessage = SequencingMessage<TYPES, I>,
>,
> TS for ViewSyncReplicaTask<TYPES, I> {}

#[derive(Snafu, Debug)]
pub struct ViewSyncTaskError {}
impl TaskErr for ViewSyncTaskError
{
}

pub type ViewSyncReplicaTaskTypes<TYPES, I> = HSTWithEvent<
    ViewSyncTaskError,
    SequencingHotShotEvent<TYPES, I>,
    ChannelStream<SequencingHotShotEvent<TYPES, I>>,
    ViewSyncReplicaTask<TYPES, I>,
>;

// pub struct ViewSyncRelayTask {}

// impl TS for ViewSyncRelayTask {

// }

impl<
        TYPES: NodeType<ConsensusType = SequencingConsensus>,
        I: NodeImplementation<
            TYPES,
            Leaf = SequencingLeaf<TYPES>,
            ConsensusMessage = SequencingMessage<TYPES, I>,
        >,
    > ViewSyncTaskState<TYPES, I>
{
    pub async fn handle_event(&mut self, event: SequencingHotShotEvent<TYPES, I>) {
        match event {
            SequencingHotShotEvent::ViewSyncMessage(message) => {
                match message {
                    ViewSyncMessageType::Certificate(certificate) => {
                        // TODO ED If task doesn't already exist, make it
                        // Don't want to create a new task if one is already running, and several different
                        // view sync certificates (1 for each phase) could trigger us to create this
                        // TODO ED Check which view it is for
                        // TODO ED Check if the cert is for an actual next view that is higher than we have now

                        if self.current_replica_task.is_none() {
                            let replica_state = ViewSyncReplicaTask { phantom: PhantomData };
                            let name = format!("View Sync Replica Task: Attempting to enter view {:?} from view {:?}", self.next_view, self.current_view);
                            let filter = FilterEvent::default();
                            let _builder = TaskBuilder::<ViewSyncReplicaTaskTypes<TYPES, I>>::new(name)
                                .register_event_stream(self.event_stream.clone(), filter)
                                .await
                                .register_state(replica_state)
                                .register_event_handler(todo!());
                        }

                        todo!()
                    }
                    ViewSyncMessageType::Vote(vote) => {
                        // TODO ED If task doesn't exist, make it (and check that it is for this relay)

                        todo!()
                    }
                }
            }
            SequencingHotShotEvent::ViewChange(new_view) => {
                if self.current_view < new_view {
                    self.current_view = new_view
                }
            }
            _ => todo!(),
        };
        return;
    }

    // Resets state once view sync has completed
    fn clear_state(&mut self) {
        todo!()
    }

    /// Filter view sync related events.
    fn filter(event: &SequencingHotShotEvent<TYPES, I>) -> bool {
        match event {
            SequencingHotShotEvent::QuorumProposalSend(_)
            | SequencingHotShotEvent::ViewSyncMessage(_)
            | SequencingHotShotEvent::Shutdown
            | SequencingHotShotEvent::ViewChange(_) => true,
            _ => false,
        }
    }

    /// Subscribe to view sync events.
    async fn subscribe(&mut self, event_stream: ChannelStream<SequencingHotShotEvent<TYPES, I>>) {
        self.filtered_event_stream = event_stream
            .subscribe(FilterEvent(Arc::new(Self::filter)))
            .await
            .0
    }
}
