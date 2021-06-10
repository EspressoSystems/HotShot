use snafu::Snafu;

/// Error type for `HotStuff`
#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
#[non_exhaustive]
pub enum HotStuffError {
    /// Failed to Message the leader in the given stage
    FailedToMessageLeader {
        /// The stage the failure occurred in
        stage: crate::data::Stage,
        /// The underlying network fault
        source: crate::networking::NetworkError,
    },
    /// Failed to broadcast a message on the network
    FailedToBroadcast {
        /// The stage the failure occurred in
        stage: crate::data::Stage,
        /// The underlying network fault
        source: crate::networking::NetworkError,
    },
    /// Bad or forged quorum certificate
    BadOrForgedQC {
        /// The stage the failure occurred in
        stage: crate::data::Stage,
        /// The bad quorum certificate
        bad_qc: crate::data::QuorumCertificate,
    },
    /// Failed to assemble a quorum certificate
    FailedToAssembleQC {
        /// The stage the error occurred in
        stage: crate::data::Stage,
        /// The underlying crypto fault
        #[snafu(source(false))]
        source: threshold_crypto::error::Error,
    },
    /// A block failed verification
    BadBlock {
        /// The stage the error occurred in
        stage: crate::data::Stage,
    },
    /// A block was not consistent with the existing state
    InconsistentBlock {
        /// The stage the error occurred in
        stage: crate::data::Stage,
    },
    /// Failure in networking layer
    NetworkFault {
        /// Underlying network fault
        source: crate::networking::NetworkError,
    },
}
