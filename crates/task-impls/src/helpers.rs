use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_broadcast::{SendError, Sender};
#[cfg(async_executor_impl = "async-std")]
use async_std::task::{spawn_blocking, JoinHandle};
use hotshot_types::{
    data::VidDisperse,
    traits::{election::Membership, node_implementation::NodeType},
    vid::vid_scheme,
};
use jf_primitives::vid::VidScheme;
#[cfg(async_executor_impl = "tokio")]
use tokio::task::{spawn_blocking, JoinHandle};

/// Cancel a task
pub async fn cancel_task<T>(task: JoinHandle<T>) {
    #[cfg(async_executor_impl = "async-std")]
    task.cancel().await;
    #[cfg(async_executor_impl = "tokio")]
    task.abort();
}

/// Helper function to send events and log errors
pub async fn broadcast_event<E: Clone + std::fmt::Debug>(event: E, sender: &Sender<E>) {
    match sender.broadcast_direct(event).await {
        Ok(None) => (),
        Ok(Some(overflowed)) => {
            tracing::error!(
                "Event sender queue overflow, Oldest event removed form queue: {:?}",
                overflowed
            );
        }
        Err(SendError(e)) => {
            tracing::warn!(
                "Event: {:?}\n Sending failed, event stream probably shutdown",
                e
            );
        }
    }
}
/// Calculate the vid disperse information from the payload given a view and membership
pub async fn calculate_vid_disperse<TYPES: NodeType>(
    txns: Vec<u8>,
    membership: Arc<TYPES::Membership>,
    view: TYPES::Time,
) -> Result<VidDisperse<TYPES>> {
    let num_nodes = membership.total_nodes();
    let vid_disperse = spawn_blocking(move || {
        vid_scheme(num_nodes).disperse(&txns).map_err(|err| anyhow!("VID disperse failure:\n\t(num_storage nodes,payload_byte_len)=({num_nodes},{})\n\terror: : {err}", txns.len()))
    })
    .await?;

    Ok(VidDisperse::from_membership(view, vid_disperse, membership))
}
