//! elections used for consensus

/// static (round robin) committee election
pub mod static_committee;

/// generic vrf based election over anything that implements `jf-primitives::vrf::Vrf`
pub mod vrf;
