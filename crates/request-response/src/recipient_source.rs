use hotshot_types::traits::signature_key::SignatureKey;

use super::request::Request;

/// A trait that allows the [`RequestResponseProtocol`] to get the recipients that a specific message should
/// expect responses from
pub trait RecipientSource<K: SignatureKey + 'static>: Send + Sync + 'static {
    /// Get all the recipients that the specific request should expect responses from
    fn get_recipients_for<R: Request>(&self, request: &R) -> Vec<K>;
}
