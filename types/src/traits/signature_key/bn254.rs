//! Demonstration implementation of the [`SignatureKey`] trait using BN254
use super::{EncodedPublicKey, EncodedSignature, SignatureKey};
/// `BN254Priv` implementation
mod bn254_priv;
/// `BN254Pub` implementation
mod bn254_pub;

pub use self::{bn254_priv::BN254Priv, bn254_pub::BN254Pub};
use jf_primitives::signatures::{bls_over_bn254::VerKey};
use jf_primitives::signatures::bls_over_bn254::{BLSOverBN254CurveSignatureScheme, KeyPair as QCKeyPair};

