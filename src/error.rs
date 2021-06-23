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
        bad_qc: crate::data::VecQuorumCertificate,
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

impl HotStuffError {
    /// Returns the stage this error happened in, if such information exists
    pub fn get_stage(&self) -> Option<crate::data::Stage> {
        match self {
            HotStuffError::FailedToMessageLeader { stage, .. }
            | HotStuffError::FailedToBroadcast { stage, .. }
            | HotStuffError::BadOrForgedQC { stage, .. }
            | HotStuffError::FailedToAssembleQC { stage, .. }
            | HotStuffError::BadBlock { stage }
            | HotStuffError::InconsistentBlock { stage } => Some(*stage),
            _ => None,
        }
    }
}
