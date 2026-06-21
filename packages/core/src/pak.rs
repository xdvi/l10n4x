//! Outer `.pak` container: DEFLATE payload + Ed25519 signature (v1).
//!
//! # Layout (big-endian)
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0      | 4    | Magic `L10P` |
//! | 4      | 4    | Version (`1`) |
//! | 8      | 4    | Payload length |
//! | 12     | N    | DEFLATE-compressed inner `L10N` binary |
//! | 12+N   | 64   | Ed25519 signature over bytes `[0..12+N)` |

extern crate alloc;
use alloc::vec::Vec;

use crate::integrity;

/// Magic bytes identifying an `l10n4x` outer pak container.
pub const PAK_MAGIC: &[u8; 4] = b"L10P";
/// Current outer pak format version.
pub const PAK_VERSION: u32 = 1;
/// Header size: magic + version + payload length.
pub const PAK_HEADER_SIZE: usize = 12;
/// Ed25519 signature length.
pub const PAK_SIGNATURE_SIZE: usize = 64;

/// Builds the unsigned container (header + DEFLATE payload).
pub fn build_unsigned(compressed: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(PAK_HEADER_SIZE + compressed.len());
    out.extend_from_slice(PAK_MAGIC);
    out.extend_from_slice(&PAK_VERSION.to_be_bytes());
    out.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    out.extend_from_slice(compressed);
    out
}

/// Appends a signature to an unsigned container.
pub fn seal(unsigned: &[u8], signature: &[u8; PAK_SIGNATURE_SIZE]) -> Vec<u8> {
    let mut out = unsigned.to_vec();
    out.extend_from_slice(signature);
    out
}

/// Parsed signed pak: `(signed_message, compressed_payload, signature)`.
pub type ParsedSignedPak<'a> = (&'a [u8], &'a [u8], &'a [u8]);

/// Parses a signed pak and returns `(signed_message, compressed_payload, signature)`.
pub fn parse_signed(data: &[u8]) -> Result<ParsedSignedPak<'_>, &'static str> {
    if data.len() < PAK_HEADER_SIZE + PAK_SIGNATURE_SIZE {
        return Err("Pak file too short");
    }
    if &data[0..4] != PAK_MAGIC {
        return Err("Invalid pak magic bytes");
    }
    let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
    if version != PAK_VERSION {
        return Err("Unsupported pak version");
    }
    let payload_len = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
    let message_end = PAK_HEADER_SIZE
        .checked_add(payload_len)
        .ok_or("Pak payload length overflow")?;
    let sig_end = message_end
        .checked_add(PAK_SIGNATURE_SIZE)
        .ok_or("Pak signature overflow")?;
    if data.len() < sig_end {
        return Err("Pak file truncated");
    }
    if data.len() != sig_end {
        return Err("Pak file has trailing bytes");
    }
    Ok((
        &data[0..message_end],
        &data[PAK_HEADER_SIZE..message_end],
        &data[message_end..sig_end],
    ))
}

/// Verifies signature and decompresses a `.pak` file into inner `L10N` binary bytes.
/// Accepts signed `L10P` files or `L10E`-encrypted envelopes (requires decrypt key).
pub fn decompress_pak(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let signed = crate::envelope::open_outer(data)?;
    decompress_signed_pak(&signed)
}

/// Verifies signature and decompresses a signed `L10P` container.
pub fn decompress_signed_pak(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    let (message, compressed, signature) = parse_signed(data)?;
    integrity::verify(message, signature)?;
    miniz_oxide::inflate::decompress_to_vec(compressed).map_err(|_| "Pak decompression failed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity;

    #[test]
    fn seal_parse_roundtrip() {
        let _lock = integrity::TEST_LOCK.lock().unwrap();

        // Reset verify key first
        use core::sync::atomic::Ordering;
        let old_ptr = integrity::VERIFY_KEY.swap(core::ptr::null_mut(), Ordering::SeqCst);
        if !old_ptr.is_null() {
            crate::reclaim::schedule_drop(old_ptr);
        }

        assert!(integrity::set_verify_key(
            &crate::test_fixtures::TEST_PUBLIC_KEY
        ));

        let body = build_unsigned(b"deflate-bytes");
        let sig: [u8; 64] = [
            110, 242, 219, 153, 169, 87, 123, 32, 227, 229, 247, 40, 63, 6, 96, 22, 64, 231, 220,
            117, 14, 137, 67, 158, 232, 94, 209, 29, 128, 215, 134, 152, 191, 233, 224, 8, 42, 27,
            15, 102, 154, 4, 173, 98, 3, 188, 99, 177, 133, 251, 248, 38, 229, 151, 45, 253, 29,
            30, 96, 58, 81, 130, 111, 10,
        ];
        let pak = seal(&body, &sig);

        let (msg, payload, sig_slice) = parse_signed(&pak).unwrap();
        assert_eq!(msg, &body);
        assert_eq!(payload, b"deflate-bytes");
        assert!(integrity::verify(msg, sig_slice).is_ok());
    }

    #[test]
    fn rejects_unsupported_version() {
        let mut bad = build_unsigned(b"x");
        bad[4..8].copy_from_slice(&99u32.to_be_bytes());
        assert!(parse_signed(&bad).is_err());
    }
}
