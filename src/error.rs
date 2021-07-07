use snafu::Snafu;

/// Error type for `PhaseLock`
#[derive(Debug, Snafu)]
#[snafu(visibility = "pub(crate)")]
#[non_exhaustive]
pub enum PhaseLockError {
    /// Failed to Message the leader in the given stage
    #[snafu(display("Failed to message leader in stage {:?}: {}", stage, source))]
    FailedToMessageLeader {
        /// The stage the failure occurred in
        stage: crate::data::Stage,
        /// The underlying network fault
        source: crate::networking::NetworkError,
    },
    /// Failed to broadcast a message on the network
    #[snafu(display("Failed to broadcast a message in stage {:?}: {}", stage, source))]
    FailedToBroadcast {
        /// The stage the failure occurred in
        stage: crate::data::Stage,
        /// The underlying network fault
        source: crate::networking::NetworkError,
    },
    /// Bad or forged quorum certificate
    #[snafu(display("Bad or forged QC in stage {:?}", stage))]
    BadOrForgedQC {
        /// The stage the failure occurred in
        stage: crate::data::Stage,
        /// The bad quorum certificate
        bad_qc: crate::data::VecQuorumCertificate,
    },
    /// Failed to assemble a quorum certificate
    #[snafu(display(
        "Failed to assemble quorum certificate in stage {:?}: {}",
        stage,
        source
    ))]
    FailedToAssembleQC {
        /// The stage the error occurred in
        stage: crate::data::Stage,
        /// The underlying crypto fault
        #[snafu(source(false))]
        source: threshold_crypto::error::Error,
    },
    /// A block failed verification
    #[snafu(display("Bad block in stage: {:?}", stage))]
    BadBlock {
        /// The stage the error occurred in
        stage: crate::data::Stage,
    },
    /// A block was not consistent with the existing state
    #[snafu(display("Inconsistent block in stage: {:?}", stage))]
    InconsistentBlock {
        /// The stage the error occurred in
        stage: crate::data::Stage,
    },
    /// Failure in networking layer
    #[snafu(display("Failure in networking layer: {}", source))]
    NetworkFault {
        /// Underlying network fault
        source: crate::networking::NetworkError,
    },
}

impl PhaseLockError {
    /// Returns the stage this error happened in, if such information exists
    pub fn get_stage(&self) -> Option<crate::data::Stage> {
        match self {
            PhaseLockError::FailedToMessageLeader { stage, .. }
            | PhaseLockError::FailedToBroadcast { stage, .. }
            | PhaseLockError::BadOrForgedQC { stage, .. }
            | PhaseLockError::FailedToAssembleQC { stage, .. }
            | PhaseLockError::BadBlock { stage }
            | PhaseLockError::InconsistentBlock { stage } => Some(*stage),
            _ => None,
        }
    }
}
