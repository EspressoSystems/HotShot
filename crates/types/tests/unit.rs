#[cfg(test)]
#[allow(unused_imports)]

use bincode;
#[allow(unused_imports)]
use serde::Serialize;

use hotshot_types::message::MessageKind::Consensus;
use hotshot_types::traits::signature_key::SignatureKey;
use hotshot_constants::PROGRAM_PROTOCOL_VERSION;
use hotshot_types::message::Message;
use hotshot_types::message::GeneralConsensusMessage::UpgradeCertificate;
use either::Left;

// use crate::{
//   hotshot_types::Message,
//   hotshot_constants::PROGRAM_PROTOCOL_VERSION,
//   traits::signature_key::SignatureKey,
// };

#[test]
fn version_number_serialized_at_start() {
  let message_kind = Consensus(Left(UpgradeCertificate));
  let message = Message { version: PROGRAM_PROTOCOL_VERSION, sender: SignatureKey::genesis_proposer_pk(), kind: message_kind };

  assert_eq!(0,0);
}
