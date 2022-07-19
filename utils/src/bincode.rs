#![allow(clippy::must_use_candidate, clippy::module_name_repetitions)]
use bincode::DefaultOptions;

/// For the wire format, we use bincode with the following options:
///   - Limit of 16KiB per message
///   - Litte endian encoding
///   - Varint encoding
///   - Reject trailing bytes
pub fn bincode_opts() -> DefaultOptions {
    bincode::DefaultOptions::new()
}
