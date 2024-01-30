use crate::{
    err,
    error::{BrokerError, Result},
};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use jf_primitives::signatures::{SignatureScheme};
use rand::{rngs::StdRng, SeedableRng};

pub fn serialize<K>(obj: &K) -> Result<Vec<u8>>
where
    K: CanonicalSerialize,
{
    // serialize verify key
    let mut bytes = Vec::new();
    err!(
        obj.serialize_uncompressed(&mut bytes),
        CryptoError,
        "failed to serialize key"
    )?;

    Ok(bytes)
}

pub fn deserialize<K>(bytes: Vec<u8>) -> Result<K>
where
    K: CanonicalDeserialize,
{
    // serialize verify key
    Ok(err!(
        K::deserialize_uncompressed(&*bytes),
        CryptoError,
        "failed to deserialize key"
    )?)
}

pub fn random_key<Scheme: SignatureScheme<PublicParameter = ()>>() -> Result<Scheme::SigningKey> {
    let mut prng = StdRng::from_entropy();
    Ok(Scheme::key_gen(&(), &mut prng).unwrap().0)
}
