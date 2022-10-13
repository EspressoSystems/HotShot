use bincode::Options;
use commit::Commitment;
use hotshot_types::{
    data::{Leaf, ViewNumber},
    traits::{
        election::{Checked, Election, ElectionError, VoteToken},
        signature_key::{EncodedPublicKey, EncodedSignature, SignatureKey},
        state::ConsensusTime,
        State,
    },
};
use hotshot_utils::{bincode::bincode_opts, hack::nll_todo};
use jf_primitives::signatures::{
    bls::{BLSSignKey, BLSSignature, BLSSignatureScheme, BLSVerKey},
    SignatureScheme,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, marker::PhantomData, num::NonZeroU64};

/// A BLS vrf public key
#[derive(Clone, Serialize, Deserialize)]
pub struct BLSPubKey {
    /// The actual public key
    pub pub_key: BLSVerKey<ark_bls12_381::Parameters>,
}

impl std::hash::Hash for BLSPubKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.to_bytes().hash(state);
    }
}

impl std::fmt::Debug for BLSPubKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BLSPubKey")
            .field("pub_key", &"A Public Key!")
            .finish()
    }
}

impl PartialEq for BLSPubKey {
    fn eq(&self, other: &Self) -> bool {
        self.to_bytes() == other.to_bytes()
    }
}

impl Eq for BLSPubKey {}

impl Ord for BLSPubKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.to_bytes().cmp(&other.to_bytes())
    }
}

impl PartialOrd for BLSPubKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.to_bytes().partial_cmp(&other.to_bytes())
    }
}

/// A BLS vrf private key
#[derive(Clone, Serialize, Deserialize)]
pub struct BLSPrivKey {
    /// The public key
    pub pub_key: BLSVerKey<ark_bls12_381::Parameters>,
    /// The private key
    pub priv_key: BLSSignKey<ark_bls12_381::Parameters>,
}

impl BLSPrivKey {
    /// Generates a new, random, `BLSPrivate` key
    ///
    /// # Panics
    ///
    /// Will panic if it was unable to generate a new key
    pub fn generate() -> Self {
        let (priv_key, pub_key) =
            BLSSignatureScheme::key_gen(&(), &mut rand::thread_rng()).unwrap();
        Self { pub_key, priv_key }
    }

    /// Turns this key into bytes
    ///
    /// # Panics
    ///
    /// Will panic if this key failed to be serialized
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode_opts().serialize(&self).unwrap()
    }

    /// Brings a key back from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        bincode_opts().deserialize(bytes).ok()
    }
}

impl std::hash::Hash for BLSPrivKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let bytes = bincode_opts().serialize(&self).unwrap();
        bytes.hash(state);
    }
}

impl Eq for BLSPrivKey {}

impl PartialOrd for BLSPrivKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        bincode_opts()
            .serialize(&self)
            .unwrap()
            .partial_cmp(&bincode_opts().serialize(other).unwrap())
    }
}

impl Ord for BLSPrivKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        bincode_opts()
            .serialize(&self)
            .unwrap()
            .cmp(&bincode_opts().serialize(other).unwrap())
    }
}

impl PartialEq for BLSPrivKey {
    fn eq(&self, other: &Self) -> bool {
        bincode_opts()
            .serialize(&self)
            .unwrap()
            .eq(&bincode_opts().serialize(other).unwrap())
    }
}

impl std::fmt::Debug for BLSPrivKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BLSPrivKey")
            .field("pub_key", &"pub key")
            .field("priv_key", &"priv key")
            .finish()
    }
}

impl SignatureKey for BLSPubKey {
    type PrivateKey = BLSPrivKey;

    fn validate(&self, signature: &EncodedSignature, data: &[u8]) -> bool {
        let x: Result<BLSSignature<ark_bls12_381::Parameters>, _> =
            bincode_opts().deserialize(&signature.0);
        match x {
            Ok(s) => {
                // First hash the data into a constant sized digest
                let hash = *blake3::hash(data).as_bytes();
                BLSSignatureScheme::verify(&(), &self.pub_key, hash, &s).is_ok()
            }
            Err(_) => false,
        }
    }

    fn sign(private_key: &Self::PrivateKey, data: &[u8]) -> EncodedSignature {
        // First hash the data into a constant sized digest
        let hash = *blake3::hash(data).as_bytes();
        // Sign it
        let signature =
            BLSSignatureScheme::sign(&(), &private_key.priv_key, hash, &mut rand::thread_rng())
                .expect("This signature shouldn't be able to fail");
        // Encode it
        let bytes = bincode_opts()
            .serialize(&signature)
            .expect("This serialization shouldn't be able to fail");
        EncodedSignature(bytes)
    }

    fn from_private(private_key: &Self::PrivateKey) -> Self {
        BLSPubKey {
            pub_key: private_key.pub_key.clone(),
        }
    }

    fn to_bytes(&self) -> EncodedPublicKey {
        EncodedPublicKey(
            bincode_opts()
                .serialize(&self)
                .expect("Serialization should not be able to fail"),
        )
    }

    fn from_bytes(bytes: &EncodedPublicKey) -> Option<Self> {
        bincode_opts().deserialize(&bytes.0).ok()
    }
}

/// Seed for the VRF
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct BLSVRFSeed(pub [u8; 32]);

impl BLSVRFSeed {
    /// Generate a random hash vrf seed
    pub fn random() -> Self {
        let mut buffer = [0_u8; 32];
        rand::thread_rng().fill_bytes(&mut buffer[..]);
        Self(buffer)
    }
    /// Generates a hash vrf seed from an arbitrary string of bytes
    pub fn from_bytes(bytes: impl AsRef<[u8]>) -> Self {
        let bytes = bytes.as_ref();
        let hash = *blake3::hash(bytes).as_bytes();
        Self(hash)
    }
}

/// A structure for a BLS vrf committee
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash)]
pub struct BLSVRF<STATE> {
    /// The internal copy of the stake table
    pub stake_table: BTreeMap<BLSPubKey, NonZeroU64>,
    /// The seed
    pub seed: BLSVRFSeed,
    /// Phantom data to store the state
    pub pd: PhantomData<STATE>,
}

impl<STATE> BLSVRF<STATE> {
    /// Calculate the leader for the view using weighted random selection
    fn select_leader(
        &self,
        table: &BTreeMap<BLSPubKey, NonZeroU64>,
        view: ViewNumber,
    ) -> BLSPubKey {
        // Convert the table into a list using the weights
        // This will always be in the same order on every machine due to the values being pulled out
        // of a `BTreeMap`, assuming the key type's `Ord` implementation is correct. If you
        // encounter issues that lead you here, check the key type's `Ord` implementation.
        let list: Vec<_> = table
            .iter()
            .flat_map(|(key, weight)| {
                vec![
                    key;
                    weight
                        .get()
                        .try_into()
                        .expect("Node has an impossibly large number of voting slots")
                ]
            })
            .collect();
        // Hash some stuff up
        // - The current view number
        // - The current stage
        // - The seed
        let mut hasher = blake3::Hasher::default();
        hasher.update(&view.to_le_bytes());
        // hasher.update(
        //     &match stage {
        //         Stage::None => 0_u64,
        //         Stage::Prepare => 1_u64,
        //         Stage::PreCommit => 2_u64,
        //         Stage::Commit => 3_u64,
        //         Stage::Decide => 4_u64,
        //     }
        //     .to_le_bytes(),
        // );
        hasher.update(&self.seed.0);
        // Extract the output
        let hash = *hasher.finalize().as_bytes();
        // Get an integer out of the first 8 bytes
        let rand = u64::from_le_bytes(hash[..8].try_into().unwrap());
        // Modulo bias doesn't matter here, as this is a testing only implementation, and there
        // can't be more nodes than will fit into memory
        let index: usize = (rand % u64::try_from(list.len()).unwrap())
            .try_into()
            .unwrap();
        // Return the result
        list[index].clone()
    }
    /// Generate a vote proof
    #[allow(dead_code)] // TODO we probably need this
    fn vote_proof(&self, key: &BLSPrivKey, view: ViewNumber, vote_index: u64) -> EncodedSignature {
        // make the buffer
        let mut buf = Vec::<u8>::new();
        buf.extend(&self.seed.0);
        buf.extend(view.to_le_bytes());
        buf.extend(vote_index.to_le_bytes());
        // Make the hash
        BLSPubKey::sign(key, &buf)
    }
    /// Generate the valid vote proofs
    #[allow(clippy::unused_self)]
    fn vote_proofs(
        &self,
        _key: &BLSPrivKey,
        _view: ViewNumber,
        _votes: u64,
    ) -> Vec<(EncodedSignature, u64)> {
        // TODO(nm-vacation): See below note in calculate_selection_threshold, this is trivial
        // comparision and filtering over the results from `vote_proof` once you have a method of
        // selecting the cuttoff
        //
        // note: the u64 in the return type is the vote index
        nll_todo()
    }
}

/// A structure for representing a vote token
// #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash)]
// pub struct BLSToken {
//     /// The public key assocaited with this token
//     pub pub_key: BLSPubKey,
//
//     /// The list of signatures
//     pub proofs: Vec<(EncodedSignature, u64)>,
// }

/// A structure for representing a validated vote token
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Hash)]
pub struct BLSToken {
    /// The public key assocaited with this token
    pub pub_key: BLSPubKey,
    /// The list of signatures
    /// TODO (ct) this is not correct, need to fix
    pub proofs: Vec<(EncodedSignature, u64)>,
    /// The number of signatures that are valid
    /// TODO (ct) this should be sorition outbput
    pub valid_proofs: u64,
}

impl VoteToken for BLSToken {
    fn vote_count(&self) -> u64 {
        nll_todo()
    }
}

impl<T, STATE> Election<BLSPubKey, T> for BLSVRF<STATE>
where
    T: ConsensusTime,
    STATE: State,
{
    type StakeTable = BTreeMap<BLSPubKey, NonZeroU64>;
    type StateType = STATE;
    type VoteTokenType = BLSToken;

    fn get_stake_table(
        &self,
        view_number: ViewNumber,
        _state: &Self::StateType,
    ) -> Self::StakeTable {
        self.stake_table.clone()
    }

    fn get_leader(&self, view_number: ViewNumber) -> BLSPubKey {
        self.select_leader(&self.stake_table, view_number)
    }

    fn check_threshold(
        &self,
        _signatures: &BTreeMap<
            EncodedPublicKey,
            (
                hotshot_types::traits::signature_key::EncodedSignature,
                Vec<u8>,
            ),
        >,
        _threshold: std::num::NonZeroUsize,
    ) -> bool {
        nll_todo()
    }

    // fn get_votes(
    //     &self,
    //     view_number: ViewNumber,
    //     pub_key: BLSPubKey,
    //     token: Self::VoteToken,
    //     _next_state: Commitment<Leaf<Self::StateType>>,
    // ) -> Option<Self::ValidatedVoteToken> {
    //     let mut validated_votes: Vec<_> = vec![];
    //     for (vote, vote_index) in token.proofs {
    //         // Recalculate the input
    //         let mut buf = Vec::<u8>::new();
    //         buf.extend(&self.seed.0);
    //         buf.extend(view_number.to_le_bytes());
    //         buf.extend(vote_index.to_le_bytes());
    //         // Verify it
    //         if token.pub_key.validate(&vote, &buf) {
    //             // TODO(nm-vacation) insert a check against your selection threshold here
    //             // todo!();
    //             validated_votes.push((vote, vote_index));
    //         }
    //     }
    //     if validated_votes.is_empty() {
    //         None
    //     } else {
    //         let len = validated_votes.len().try_into().unwrap();
    //         Some(ValidatedBLSToken {
    //             pub_key,
    //             proofs: validated_votes,
    //             valid_proofs: len,
    //         })
    //     }
    // }

    // fn get_vote_count(&self, token: &Self::ValidatedVoteToken) -> u64 {
    //     token.valid_proofs
    // }
    //
    fn make_vote_token(
        &self,
        view_number: ViewNumber,
        private_key: &<BLSPubKey as SignatureKey>::PrivateKey,
        _next_state: Commitment<Leaf<Self::StateType>>,
    ) -> Result<Option<Self::VoteTokenType>, ElectionError> {
        nll_todo()
        // let pub_key = BLSPubKey::from_private(private_key);
        // if let Some(votes) = self.stake_table.get(&pub_key) {
        //     // Get the votes for our self
        //     let hashes = self.vote_proofs(private_key, view_number, votes.get());
        //     if hashes.is_empty() {
        //         None
        //     } else {
        //         Some(BLSToken {
        //             pub_key: BLSPubKey::from_private(private_key),
        //             proofs: hashes,
        //         })
        //     }
        // } else {
        //     None
        // }
    }

    fn validate_vote_token(
        &self,
        view_number: ViewNumber,
        pub_key: BLSPubKey,
        token: Checked<Self::VoteTokenType>,
        next_state: commit::Commitment<hotshot_types::data::Leaf<Self::StateType>>,
    ) -> Result<hotshot_types::traits::election::Checked<Self::VoteTokenType>, ElectionError> {
        nll_todo()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    // Basic smoke test
    #[test]
    fn signature_should_validate() {
        // Get some data to test sign with
        let mut data = [0_u8; 64];
        rand::thread_rng().fill_bytes(&mut data);

        // Get a key to sign it with
        let priv_key = BLSPrivKey::generate();
        // And the matching public key
        let pub_key = BLSPubKey::from_private(&priv_key);

        // Sign the data with it
        let signature = BLSPubKey::sign(&priv_key, &data);
        // Verify the signature
        assert!(pub_key.validate(&signature, &data));
    }

    // Make sure serialization round trip works
    #[test]
    fn serialize_key() {
        // Get a private key
        let priv_key = BLSPrivKey::generate();
        // And the matching public key
        let pub_key = BLSPubKey::from_private(&priv_key);

        // Convert the private key to bytes and back, then verify equality
        let priv_key_bytes = priv_key.to_bytes();
        let priv_key_2 = BLSPrivKey::from_bytes(&priv_key_bytes).expect("Failed to deser key");
        assert!(priv_key == priv_key_2);

        // Convert the public key to bytes and back, then verify equality
        let pub_key_bytes = pub_key.to_bytes();
        let pub_key_2 = BLSPubKey::from_bytes(&pub_key_bytes).expect("Failed to deser key");
        assert_eq!(pub_key, pub_key_2);

        // Serialize the public key and back, then verify equality
        let serialized = bincode::serialize(&pub_key).expect("Failed to ser key");
        let pub_key_2: BLSPubKey = bincode::deserialize(&serialized).expect("Failed to deser key");
        assert_eq!(pub_key, pub_key_2);
    }
}
