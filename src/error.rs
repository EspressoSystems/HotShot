use snafu::Snafu;

/// Error type for `HotStuff`
#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
pub enum HotStuffError {
    #[snafu(display("Sanity Check Failed"))]
    SanityCheckFailure,
    BlockAlreadyDelivered,
    NewHQCInvalid,
    NetworkFault {
        source: crate::NetworkError,
    },
}
