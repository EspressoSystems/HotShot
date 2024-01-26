//! Utilities for reading version number

use hotshot_constants::Version;

/// Read the version number from a message (passed a byte vector),
/// returning None if there are not enough bytes.
#[must_use]
#[allow(clippy::module_name_repetitions)]
pub fn read_version(message: &[u8]) -> Option<Version> {
    let maybe_bytes_major: Result<[u8; 2], _> = message[0..2].try_into();
    let maybe_bytes_minor: Result<[u8; 2], _> = message[2..4].try_into();

    match (maybe_bytes_major, maybe_bytes_minor) {
        (Ok(bytes_major), Ok(bytes_minor)) => {
            let major = u16::from_le_bytes(bytes_major);
            let minor = u16::from_le_bytes(bytes_minor);
            Some(Version { major, minor })
        }
        _ => None,
    }
}
