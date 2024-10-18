use bincode::Options;
use cdn_broker::reexports::crypto::signature::{Serializable, SignatureScheme};
use hotshot_types::{traits::signature_key::SignatureKey, utils::bincode_opts};

/// A wrapped `SignatureKey`. We need to implement the Push CDN's `SignatureScheme`
/// trait in order to sign and verify messages to/from the CDN.
#[derive(Clone, Eq, PartialEq)]
pub struct WrappedSignatureKey<T: SignatureKey + 'static>(pub T);
impl<T: SignatureKey> SignatureScheme for WrappedSignatureKey<T> {
    type PrivateKey = T::PrivateKey;
    type PublicKey = Self;

    /// Sign a message of arbitrary data and return the serialized signature
    fn sign(
        private_key: &Self::PrivateKey,
        namespace: &str,
        message: &[u8],
    ) -> anyhow::Result<Vec<u8>> {
        // Combine the namespace and message into a single byte array
        let message = [namespace.as_bytes(), message].concat();

        let signature = T::sign(private_key, &message)?;
        Ok(bincode_opts().serialize(&signature)?)
    }

    /// Verify a message of arbitrary data and return the result
    fn verify(
        public_key: &Self::PublicKey,
        namespace: &str,
        message: &[u8],
        signature: &[u8],
    ) -> bool {
        // Deserialize the signature
        let signature: T::PureAssembledSignatureType = match bincode_opts().deserialize(signature) {
            Ok(key) => key,
            Err(_) => return false,
        };

        // Combine the namespace and message into a single byte array
        let message = [namespace.as_bytes(), message].concat();

        public_key.0.validate(&signature, &message)
    }
}

/// We need to implement the `Serializable` so the Push CDN can serialize the signatures
/// and public keys and send them over the wire.
impl<T: SignatureKey> Serializable for WrappedSignatureKey<T> {
    fn serialize(&self) -> anyhow::Result<Vec<u8>> {
        Ok(self.0.to_bytes())
    }

    fn deserialize(serialized: &[u8]) -> anyhow::Result<Self> {
        Ok(WrappedSignatureKey(T::from_bytes(serialized)?))
    }
}
