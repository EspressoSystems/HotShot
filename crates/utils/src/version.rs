//! Utilities for reading version number

use hotshot_constants::Version;

/// Read the version number from a message (passed a byte vector).
///
/// # Panics
///
/// Panics if the message is too short.
#[must_use]
#[allow(clippy::module_name_repetitions)]
pub fn read_version(message: &[u8]) -> Version {
    let major = u16::from_le_bytes(
        message[0..2]
            .try_into()
            .expect("Insufficient bytes; cannot read major version."),
    );
    let minor = u16::from_le_bytes(
        message[2..4]
            .try_into()
            .expect("Insufficient bytes; cannot read minor version."),
    );

    Version { major, minor }
}
