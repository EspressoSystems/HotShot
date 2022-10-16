//! TODO(vko) document

/// TODO(vko) document
// pub mod bls_vrf;
/// TODO(vko) document
pub mod static_committee;

/// Hash based psuedo-vrf that simulates the properties of a VRF using an HMAC with a revealed
/// private key. This is for testing purposes only and is designed to be insecure
// pub mod stub;

/// generic over anything that implements jf-primitives::vrf::Vrf
pub mod vrf;
