use std::{
    io::{Cursor, Read, Write},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use hotshot_types::traits::signature_key::SignatureKey;

use super::{request::Request, RequestHash, Serializable};

/// The message type for the request-response protocol. Can either be a request or a response
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message<R: Request, K: SignatureKey> {
    /// A request
    Request(RequestMessage<R, K>),
    /// A response
    Response(ResponseMessage<R>),
}

/// A request message, which includes the requester's public key, the request's signature, and the request itself
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestMessage<R: Request, K: SignatureKey> {
    /// The requester's public key
    pub public_key: K,
    /// The requester's signature over the [request content + timestamp]
    pub signature: K::PureAssembledSignatureType,
    /// The timestamp of when the request was sent (in seconds since the Unix epoch)
    pub timestamp_unix_seconds: u64,
    /// The request's content
    pub content: R,
}

impl<R: Request, K: SignatureKey> RequestMessage<R, K> {
    /// Create a new signed request message
    pub fn new_signed(public_key: K, private_key: K::PrivateKey, content: R) -> Result<Self>
    where
        <K as SignatureKey>::SignError: 'static,
    {
        // Get the current timestamp
        let timestamp_unix_seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_secs();

        // Concatenate the content and timestamp
        let timestamped_content = [
            &content
                .to_bytes()
                .with_context(|| "failed to serialize request content")?
                .as_slice(),
            timestamp_unix_seconds.to_le_bytes().as_slice(),
        ]
        .concat();

        // Sign the content with the private key
        let signature = K::sign(&private_key, &timestamped_content)
            .with_context(|| "failed to sign message")?;

        // Return the newly signed request message
        Ok(RequestMessage {
            public_key,
            signature,
            timestamp_unix_seconds,
            content,
        })
    }

    /// Validate the request message, checking the signature and the timestamp
    pub fn validate(&self, incoming_request_ttl: Duration) -> Result<()> {
        // Check the signature over the request content and timestamp
        if !self.public_key.validate(
            &self.signature,
            &[
                self.content.to_bytes()?,
                self.timestamp_unix_seconds.to_le_bytes().to_vec(),
            ]
            .concat(),
        ) {
            return Err(anyhow::anyhow!("invalid request signature"));
        }

        // Check if the request is too old
        if self
            .timestamp_unix_seconds
            .saturating_add(incoming_request_ttl.as_secs())
            < SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time went backwards")
                .as_secs()
        {
            return Err(anyhow::anyhow!("request is too old"));
        }

        Ok(())
    }
}

/// A response message, which includes the hash of the request we're responding to and the response itself
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseMessage<R: Request> {
    /// The hash of the request we're responding to
    pub request_hash: RequestHash,
    /// The actual content of the response
    pub content: R::Response,
}

/// A blanket implementation of the [`Serializable`] trait for any [`Message`]
impl<R: Request, K: SignatureKey> Serializable for Message<R, K> {
    /// Converts any [`Message`] to bytes
    fn to_bytes(&self) -> Result<Vec<u8>> {
        // Create a buffer for the bytes
        let mut bytes = Vec::new();

        // Convert the message to bytes based on the type. By default it is just type-prefixed
        match self {
            Message::Request(request_message) => {
                // Write the request type
                bytes.push(0);

                // Write the request content
                bytes.extend_from_slice(request_message.to_bytes()?.as_slice());
            }
            Message::Response(response_message) => {
                // Write the response type
                bytes.push(1);

                // Write the response content
                bytes.extend_from_slice(response_message.to_bytes()?.as_slice());
            }
        };

        Ok(bytes)
    }

    /// Convert bytes to a [`Message`]
    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // Create a cursor so we can easily read the bytes in order
        let mut bytes = Cursor::new(bytes);

        // Get the message type
        let type_byte = bytes.read_u8()?;

        // Deserialize the message based on the type
        match type_byte {
            0 => {
                // Read the `RequestMessage`
                Ok(Message::Request(RequestMessage::from_bytes(&read_to_end(
                    &mut bytes,
                )?)?))
            }
            1 => {
                // Read the `ResponseMessage`
                Ok(Message::Response(ResponseMessage::from_bytes(
                    &read_to_end(&mut bytes)?,
                )?))
            }
            _ => Err(anyhow::anyhow!("invalid message type")),
        }
    }
}

impl<R: Request, K: SignatureKey> Serializable for RequestMessage<R, K> {
    fn to_bytes(&self) -> Result<Vec<u8>> {
        // Create a buffer for the bytes
        let mut bytes = Vec::new();

        // Write the public key (length-prefixed)
        write_length_prefixed(&mut bytes, self.public_key.to_bytes())?;

        // Write the signature (length-prefixed)
        write_length_prefixed(&mut bytes, bincode::serialize(&self.signature)?)?;

        // Write the timestamp
        bytes.write_all(&self.timestamp_unix_seconds.to_le_bytes())?;

        // Write the actual request content
        bytes.write_all(self.content.to_bytes()?.as_slice())?;

        Ok(bytes)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // Create a cursor so we can easily read the bytes in order
        let mut bytes = Cursor::new(bytes);

        // Read the public key (length-prefixed)
        let public_key = K::from_bytes(&read_length_prefixed(&mut bytes)?)?;

        // Read the signature (length-prefixed)
        let signature = bincode::deserialize(&read_length_prefixed(&mut bytes)?)?;

        // Read the timestamp as a [`u64`]
        let timestamp = bytes.read_u64::<LittleEndian>()?;

        // Deserialize the request content
        let content = R::from_bytes(&read_to_end(&mut bytes)?)?;

        Ok(Self {
            public_key,
            signature,
            timestamp_unix_seconds: timestamp,
            content,
        })
    }
}

impl<R: Request> Serializable for ResponseMessage<R> {
    fn to_bytes(&self) -> Result<Vec<u8>> {
        // Create a buffer for the bytes
        let mut bytes = Vec::new();

        // Write the request hash (length-prefixed)
        bytes.write_u64::<LittleEndian>(self.request_hash)?;

        // Write the response content
        bytes.write_all(self.content.to_bytes()?.as_slice())?;

        Ok(bytes)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // Create a buffer for the bytes
        let mut bytes = Cursor::new(bytes);

        // Read the request hash as a [`u64`]
        let request_hash = bytes.read_u64::<LittleEndian>()?;

        // Read the response content to the end
        let content = R::Response::from_bytes(&read_to_end(&mut bytes)?)?;

        Ok(Self {
            request_hash,
            content,
        })
    }
}

/// A helper function to write a length-prefixed value to a writer
fn write_length_prefixed<W: Write>(writer: &mut W, value: Vec<u8>) -> Result<()> {
    // Write the length of the value as a u32
    writer.write_u32::<LittleEndian>(value.len() as u32)?;

    // Write the (already serialized) value
    writer.write_all(&value)?;
    Ok(())
}

/// A helper function to read a length-prefixed value from a reader
fn read_length_prefixed<R: Read>(reader: &mut R) -> Result<Vec<u8>> {
    // Read the length of the value as a u32
    let length = reader.read_u32::<LittleEndian>()?;

    // Read the value
    let mut value = vec![0; length as usize];
    reader.read_exact(&mut value)?;
    Ok(value)
}

/// A helper function to read to the end of the reader
fn read_to_end<R: Read>(reader: &mut R) -> Result<Vec<u8>> {
    let mut value = Vec::new();
    reader.read_to_end(&mut value)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use hotshot_types::signature_key::BLSPubKey;
    use rand::Rng;

    use crate::traits::implementations::request::Response;

    use super::*;

    // A testing implementation of the [`Serializable`] trait for [`Vec<u8>`]
    impl Serializable for Vec<u8> {
        fn to_bytes(&self) -> Result<Vec<u8>> {
            Ok(self.clone())
        }
        fn from_bytes(bytes: &[u8]) -> Result<Self> {
            Ok(bytes.to_vec())
        }
    }

    /// A testing implementation of the [`Request`] trait for [`Vec<u8>`]
    impl Request for Vec<u8> {
        type Response = Vec<u8>;
        fn is_valid(&self) -> bool {
            true
        }
    }

    /// A testing implementation of the [`Response`] trait for [`Vec<u8>`]
    impl Response<Vec<u8>> for Vec<u8> {
        fn is_valid(&self, _request: &Vec<u8>) -> bool {
            true
        }
    }

    /// Tests that properly signed requests are validated correctly and that invalid requests
    /// (bad timestamp/signature) are rejected
    #[test]
    fn test_request_validation() {
        // Create some RNG
        let mut rng = rand::thread_rng();

        for _ in 0..100 {
            // Create a random keypair
            let (public_key, private_key) =
                BLSPubKey::generated_from_seed_indexed([1; 32], rng.gen::<u64>());

            // Create a valid request with some random content
            let mut request = RequestMessage::new_signed(
                public_key,
                private_key,
                vec![rng.gen::<u8>(); rng.gen_range(1..10000)],
            )
            .expect("Failed to create signed request");

            let (should_be_valid, request_ttl) = match rng.gen_range(0..4) {
                0 => (true, Duration::from_secs(1)),

                1 => {
                    // Alter the content
                    request.content[0] = !request.content[0];

                    // It should not be valid anymore
                    (false, Duration::from_secs(1))
                }

                2 => {
                    // Alter the timestamp
                    request.timestamp_unix_seconds += 1000;

                    // It should not be valid anymore
                    (false, Duration::from_secs(1))
                }

                3 => {
                    // Change the request ttl to be 0. This should make the request
                    // invalid immediately
                    (true, Duration::from_secs(0))
                }

                _ => unreachable!(),
            };

            // Validate the request
            assert_eq!(request.validate(request_ttl).is_ok(), should_be_valid);
        }
    }

    /// Tests that messages are serialized and deserialized correctly
    #[test]
    fn test_message_parity() {
        for _ in 0..100 {
            // Create some RNG
            let mut rng = rand::thread_rng();

            // Generate a random message type
            let is_request = rng.gen::<u8>() % 2 == 0;

            // The request content will be a random vector of bytes
            let content = vec![rng.gen::<u8>(); rng.gen_range(0..10000)];

            // Create a message
            let message = match is_request {
                true => {
                    // Create a random keypair
                    let (public_key, private_key) =
                        BLSPubKey::generated_from_seed_indexed([1; 32], rng.gen::<u64>());

                    // Create a new signed request
                    let request = RequestMessage::new_signed(public_key, private_key, content)
                        .expect("Failed to create signed request");

                    Message::Request(request)
                }
                false => Message::Response(ResponseMessage {
                    request_hash: rng.gen::<u64>(),
                    content: vec![rng.gen::<u8>(); rng.gen_range(0..10000)],
                }),
            };

            // Serialize the message
            let serialized = message.to_bytes().expect("Failed to serialize message");
            println!("serialized: {:?}", serialized);

            // Deserialize the message
            let deserialized =
                Message::from_bytes(&serialized).expect("Failed to deserialize message");

            // Assert that the deserialized message is the same as the original message
            assert_eq!(message, deserialized);
        }
    }

    //// Tests that length-prefixed values are read and written correctly
    #[test]
    fn test_length_prefix_parity() {
        // Create some RNG
        let mut rng = rand::thread_rng();

        for _ in 0..100 {
            // Create a buffer to test over
            let mut bytes = Vec::new();

            // Generate the value to test over
            let value = vec![rng.gen::<u8>(); rng.gen_range(0..10000)];

            // Write the length-prefixed value
            write_length_prefixed(&mut bytes, value).unwrap();

            // Create a reader from the bytes
            let mut reader = Cursor::new(bytes);

            // Read the length-prefixed value
            let value = read_length_prefixed(&mut reader).unwrap();
            assert_eq!(value, value);
        }
    }
}
