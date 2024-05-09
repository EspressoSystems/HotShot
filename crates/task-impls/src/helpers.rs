use std::sync::Arc;

#[cfg(async_executor_impl = "async-std")]
use async_std::task::spawn_blocking;
use hotshot_types::{
    data::VidDisperse,
    traits::{election::Membership, node_implementation::NodeType},
    vid::{vid_scheme, VidPrecomputeData},
};
use jf_primitives::vid::{precomputable::Precomputable, VidScheme};
#[cfg(async_executor_impl = "tokio")]
use tokio::task::spawn_blocking;

/// Calculate the vid disperse information from the payload given a view and membership
///
/// # Panics
/// Panics if the VID calculation fails, this should not happen.
#[allow(clippy::panic)]
pub async fn calculate_vid_disperse<TYPES: NodeType>(
    txns: Arc<[u8]>,
    membership: &Arc<TYPES::Membership>,
    view: TYPES::Time,
) -> VidDisperse<TYPES> {
    let num_nodes = membership.total_nodes();
    let vid_disperse = spawn_blocking(move || {
        vid_scheme(num_nodes).disperse(&txns).unwrap_or_else(|err| panic!("VID precompute disperse failure:(num_storage nodes,payload_byte_len)=({num_nodes},{}) error: {err}", txns.len()))
    })
    .await;
    #[cfg(async_executor_impl = "tokio")]
    // Unwrap here will just propagate any panic from the spawned task, it's not a new place we can panic.
    let vid_disperse = vid_disperse.unwrap();

    VidDisperse::from_membership(view, vid_disperse, membership.as_ref())
}

/// Calculate the vid disperse information from the payload given a view and membership, and precompute data from builder
///
/// # Panics
/// Panics if the VID calculation fails, this should not happen.
#[allow(clippy::panic)]
pub async fn calculate_vid_disperse_using_precompute_data<TYPES: NodeType>(
    txns: Arc<[u8]>,
    membership: &Arc<TYPES::Membership>,
    view: TYPES::Time,
    pre_compute_data: VidPrecomputeData,
) -> VidDisperse<TYPES> {
    let num_nodes = membership.total_nodes();
    let vid_disperse = spawn_blocking(move || {
        vid_scheme(num_nodes).disperse_precompute(&txns, &pre_compute_data).unwrap_or_else(|err| panic!("VID precompute disperse failure:(num_storage nodes,payload_byte_len)=({num_nodes},{}) error: {err}", txns.len()))
    })
    .await;
    #[cfg(async_executor_impl = "tokio")]
    // Unwrap here will just propagate any panic from the spawned task, it's not a new place we can panic.
    let vid_disperse = vid_disperse.unwrap();

    VidDisperse::from_membership(view, vid_disperse, membership.as_ref())
}

/// Utilities to print anyhow logs.
pub trait AnyhowTracing {
    /// Print logs as debug
    fn err_as_debug(self);
}

impl<T> AnyhowTracing for anyhow::Result<T> {
    fn err_as_debug(self) {
        let _ = self.inspect_err(|e| tracing::debug!("{}", format!("{:?}", e)));
    }
}
