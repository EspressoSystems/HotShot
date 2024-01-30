use common::crypto;
use jf_primitives::signatures::bls_over_bn254::BLSOverBN254CurveSignatureScheme as BLS;

// this example generates a random key in hex format
pub fn main() {
    println!(
        "{}",
        hex::encode(crypto::serialize(&crypto::random_key::<BLS>().unwrap()).unwrap())
    );
}
