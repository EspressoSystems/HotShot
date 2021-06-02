use snafu::Snafu;

/// Error type for `HotStuff`
#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
pub enum HotStuffError {
    /// Sanity check failed
    SanityCheckFailure,
    /// Attempted to deliver a block more than once
    BlockAlreadyDelivered,
    /// New high qc is not valid
    NewHQCInvalid,
    /// Failure in networking layer
    NetworkFault {
        /// Underlying network fault
        source: crate::networking::NetworkError,
    },
}
