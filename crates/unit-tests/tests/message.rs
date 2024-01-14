#[cfg(test)]
use std::marker::PhantomData;

use bincode;
use commit::Committable;
use either::Left;


use hotshot_testing::node_types::TestTypes;

use hotshot_types::{
    message::{GeneralConsensusMessage, Message, MessageKind, SequencingMessage},
    signature_key::BLSPubKey,
    simple_certificate::SimpleCertificate,
    simple_vote::ViewSyncCommitData,
    traits::{signature_key::SignatureKey, state::ConsensusTime},
};

#[test]
// Checks that the current program protocol version
// correctly appears at the start of a serialized messaged.
fn version_number_at_start_of_serialization() {
    let sender = BLSPubKey::generated_from_seed_indexed([0u8; 32], 0).0;
    let view_number = ConsensusTime::new(17);
    // The version we set for the message
    let major_version = 37;
    let minor_version = 17;
    // The specific data we attach to our message shouldn't affect the serialization,
    // we're using ViewSyncCommitData for simplicity.
    let data: ViewSyncCommitData<TestTypes> = ViewSyncCommitData {
        relay: 37,
        round: view_number,
    };
    let simple_certificate = SimpleCertificate {
        data: data.clone(),
        vote_commitment: data.commit(),
        view_number: view_number,
        signatures: None,
        is_genesis: false,
        _pd: PhantomData,
    };
    let message = Message {
        major_version,
        minor_version,
        sender,
        kind: MessageKind::Consensus(SequencingMessage(Left(
            GeneralConsensusMessage::ViewSyncCommitCertificate(simple_certificate),
        ))),
    };
    let serialized_message: Vec<u8> = bincode::serialize(&message).unwrap();
    // The versions we've read from the message
    let major_version_read = u32::from_le_bytes(serialized_message[..4].try_into().unwrap());
    let minor_version_read = u32::from_le_bytes(serialized_message[4..8].try_into().unwrap());

    assert_eq!(major_version, major_version_read);
    assert_eq!(minor_version, minor_version_read);
}
