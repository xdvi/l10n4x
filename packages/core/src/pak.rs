//! Outer `.pak` container: zstd payload + Ed25519 signature (v1).
//!
//! # Layout (big-endian)
//!
//! ## Original (v1, 12‑byte header)
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0      | 4    | Magic `L10P` |
//! | 4      | 4    | Version (`1`) |
//! | 8      | 4    | Payload length |
//! | 12     | N    | zstd-compressed inner `L10N` binary |
//! | 12+N   | 64   | Ed25519 signature over bytes `[0..12+N)` |
//!
//! ## Extended (v1, 16‑byte header)
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0      | 4    | Magic `L10P` |
//! | 4      | 4    | Version (`1`) |
//! | 8      | 4    | Flags (bit 0 = parent_locale present) |
//! | 12     | 4    | Payload length |
//! | 16     | 1    | Parent locale byte length (if flag 0 set) |
//! | 17     | P    | Parent locale UTF‑8 bytes (if flag 0 set) |
//! | 16+P   | N    | zstd-compressed inner `L10N` binary |
//! | 16+P+N | 64   | Ed25519 signature over bytes `[0..16+P+N)` |

extern crate alloc;
use alloc::vec::Vec;

use crate::error::CoreResult;
use crate::integrity;

/// Magic bytes identifying an `l10n4x` outer pak container.
pub const PAK_MAGIC: &[u8; 4] = b"L10P";
/// Current outer pak format version.
pub const PAK_VERSION: u32 = 1;
/// Header size: magic + version + payload length.
pub const PAK_HEADER_SIZE: usize = 12;
/// Extended header size: magic + version + flags + payload length.
pub const PAK_EXT_HEADER_SIZE: usize = 16;
/// Ed25519 signature length.
pub const PAK_SIGNATURE_SIZE: usize = 64;

/// Builds the unsigned container (extended header + zstd payload).
pub fn build_unsigned(compressed: &[u8], parent_locale: Option<&str>) -> Vec<u8> {
    let mut flags: u32 = 0;
    let parent_bytes = parent_locale.map(|p| p.as_bytes()).unwrap_or(b"");
    if !parent_bytes.is_empty() {
        flags |= 1;
    }
    let extra = if flags & 1 != 0 {
        1 + parent_bytes.len()
    } else {
        0
    };
    let mut out = Vec::with_capacity(PAK_EXT_HEADER_SIZE + extra + compressed.len());
    out.extend_from_slice(PAK_MAGIC);
    out.extend_from_slice(&PAK_VERSION.to_be_bytes());
    out.extend_from_slice(&flags.to_be_bytes());
    out.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    if flags & 1 != 0 {
        out.push(parent_bytes.len() as u8);
        out.extend_from_slice(parent_bytes);
    }
    out.extend_from_slice(compressed);
    out
}

/// Appends a signature to an unsigned container.
///
/// Reserves the exact final capacity up front (`unsigned + signature`) so the result is a
/// single allocation with no intermediate realloc, rather than `to_vec()` (exact-size) followed
/// by an `extend` that forces a second allocation and a full copy of the payload.
pub fn seal(unsigned: &[u8], signature: &[u8; PAK_SIGNATURE_SIZE]) -> Vec<u8> {
    let mut out = Vec::with_capacity(unsigned.len() + PAK_SIGNATURE_SIZE);
    out.extend_from_slice(unsigned);
    out.extend_from_slice(signature);
    out
}

/// Parsed signed pak: `(signed_message, compressed_payload, signature, parent_locale)`.
pub type ParsedSignedPak<'a> = (&'a [u8], &'a [u8], &'a [u8], Option<&'a str>);

/// Parses a signed pak (old 12‑byte or new 16‑byte header) and returns
/// `(signed_message, compressed_payload, signature, parent_locale)`.
pub fn parse_signed(data: &[u8]) -> CoreResult<ParsedSignedPak<'_>> {
    use crate::CoreError::*;
    if data.len() < PAK_HEADER_SIZE + PAK_SIGNATURE_SIZE {
        return Err(BufferTooShort("Pak file too short"));
    }
    if &data[0..4] != PAK_MAGIC {
        return Err(InvalidMagic("Invalid pak magic bytes"));
    }
    let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
    if version != PAK_VERSION {
        return Err(UnsupportedVersion(version));
    }
    let (_flags, payload_offset, payload_len, parent) =
        if data.len() >= PAK_EXT_HEADER_SIZE + PAK_SIGNATURE_SIZE {
            let raw_flags = u32::from_be_bytes(data[8..12].try_into().unwrap());
            if raw_flags & !1 == 0 {
                let payload_len = u32::from_be_bytes(data[12..16].try_into().unwrap()) as usize;
                let fits = payload_len > 0
                    && PAK_EXT_HEADER_SIZE + payload_len + PAK_SIGNATURE_SIZE <= data.len();
                if fits {
                    let mut pos = PAK_EXT_HEADER_SIZE;
                    let parent = if raw_flags & 1 != 0 {
                        if pos + 1 > data.len() {
                            return Err(BufferTooShort("parent len truncated"));
                        }
                        let parent_len = data[pos] as usize;
                        pos += 1;
                        if pos + parent_len > data.len() {
                            return Err(BufferTooShort("parent bytes truncated"));
                        }
                        let s = core::str::from_utf8(&data[pos..pos + parent_len])
                            .map_err(|_| InvalidFormat("Invalid parent_locale UTF-8"))?;
                        pos += parent_len;
                        Some(s)
                    } else {
                        None
                    };
                    (raw_flags, pos, payload_len, parent)
                } else {
                    let payload_len = raw_flags as usize;
                    (0u32, PAK_HEADER_SIZE, payload_len, None)
                }
            } else {
                let payload_len = raw_flags as usize;
                (0u32, PAK_HEADER_SIZE, payload_len, None)
            }
        } else {
            let payload_len = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
            (0u32, PAK_HEADER_SIZE, payload_len, None)
        };
    let message_end = payload_offset
        .checked_add(payload_len)
        .ok_or(Overflow("Pak payload length overflow"))?;
    let sig_end = message_end
        .checked_add(PAK_SIGNATURE_SIZE)
        .ok_or(Overflow("Pak signature overflow"))?;
    if data.len() < sig_end {
        return Err(BufferTooShort("Pak file truncated"));
    }
    Ok((
        &data[0..message_end],
        &data[payload_offset..message_end],
        &data[message_end..sig_end],
        parent,
    ))
}

/// Verifies signature and decompresses a `.pak` file into inner `L10N` binary bytes.
/// Accepts signed `L10P` files or `L10E`-encrypted envelopes (requires decrypt key).
pub fn decompress_pak(data: &[u8]) -> CoreResult<Vec<u8>> {
    let signed = crate::envelope::open_outer(data)?;
    decompress_signed_pak(&signed)
}

/// Decompresses a raw zstd payload into inner `L10N` binary bytes.
pub(crate) fn decompress_zstd_payload(compressed: &[u8]) -> CoreResult<Vec<u8>> {
    use ruzstd::frame_decoder::BlockDecodingStrategy;
    use ruzstd::io::Read;

    let mut decoder = ruzstd::FrameDecoder::new();
    let mut reader = compressed;
    decoder
        .reset(&mut reader)
        .map_err(|_| crate::CoreError::IoError("zstd decompression: init failed"))?;
    decoder
        .decode_blocks(&mut reader, BlockDecodingStrategy::All)
        .map_err(|_| crate::CoreError::IoError("zstd decompression: decode failed"))?;
    let mut output = Vec::with_capacity(compressed.len().saturating_mul(4).max(4096));
    let mut buf = [0u8; 4096];
    loop {
        let n = decoder
            .read(&mut buf)
            .map_err(|_| crate::CoreError::IoError("zstd decompression: read failed"))?;
        if n == 0 {
            break;
        }
        output.extend_from_slice(&buf[..n]);
    }
    Ok(output)
}

/// Verifies signature and decompresses a signed `L10P` container.
pub fn decompress_signed_pak(data: &[u8]) -> CoreResult<Vec<u8>> {
    let (message, compressed, signature, _parent) = parse_signed(data)?;
    integrity::verify(message, signature)?;
    decompress_zstd_payload(compressed)
}

/// Extracts the optional parent locale from a raw pak byte slice.
/// Returns `None` for old-format files, truncated buffers, or when the flag is unset.
pub fn get_parent_locale(data: &[u8]) -> Option<&str> {
    if data.len() < PAK_EXT_HEADER_SIZE {
        return None;
    }
    if &data[0..4] != PAK_MAGIC {
        return None;
    }
    let version = u32::from_be_bytes(data[4..8].try_into().ok()?);
    if version != PAK_VERSION {
        return None;
    }
    if data.len() < PAK_EXT_HEADER_SIZE + 1 {
        return None;
    }
    let flags = u32::from_be_bytes(data[8..12].try_into().ok()?);
    if flags & 1 == 0 {
        return None;
    }
    let parent_len = *data.get(PAK_EXT_HEADER_SIZE)? as usize;
    if parent_len == 0 || PAK_EXT_HEADER_SIZE + 1 + parent_len > data.len() {
        return None;
    }
    core::str::from_utf8(&data[PAK_EXT_HEADER_SIZE + 1..PAK_EXT_HEADER_SIZE + 1 + parent_len]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "alloc")]
    #[test]
    fn seal_parse_roundtrip() {
        use crate::integrity;
        use ed25519_dalek::Signer;

        let _lock = integrity::TEST_LOCK.lock().unwrap();

        // Reset verify key first
        use core::sync::atomic::Ordering;
        let old_ptr = integrity::VERIFY_KEY.swap(core::ptr::null_mut(), Ordering::SeqCst);
        if !old_ptr.is_null() {
            crate::reclaim::schedule_drop(old_ptr);
        }

        let seed = [42u8; 32];
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        assert!(integrity::set_verify_key(verifying_key.as_bytes()));

        let body = build_unsigned(b"zstd-payload", None);
        let sig = signing_key.sign(&body).to_bytes();
        let pak = seal(&body, &sig);

        let (msg, payload, sig_slice, parent) = parse_signed(&pak).unwrap();
        assert_eq!(msg, &body);
        assert_eq!(payload, b"zstd-payload");
        assert!(integrity::verify(msg, sig_slice).is_ok());
        assert_eq!(parent, None);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn seal_parse_roundtrip_with_parent() {
        use crate::integrity;
        use ed25519_dalek::Signer;

        let _lock = integrity::TEST_LOCK.lock().unwrap();

        use core::sync::atomic::Ordering;
        let old_ptr = integrity::VERIFY_KEY.swap(core::ptr::null_mut(), Ordering::SeqCst);
        if !old_ptr.is_null() {
            crate::reclaim::schedule_drop(old_ptr);
        }

        let seed = [99u8; 32];
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        assert!(integrity::set_verify_key(verifying_key.as_bytes()));

        let body = build_unsigned(b"data-with-parent", Some("es"));
        let sig = signing_key.sign(&body).to_bytes();
        let pak = seal(&body, &sig);

        let (msg, payload, sig_slice, parent) = parse_signed(&pak).unwrap();
        assert_eq!(msg, &body);
        assert_eq!(payload, b"data-with-parent");
        assert!(integrity::verify(msg, sig_slice).is_ok());
        assert_eq!(parent, Some("es"));
        assert_eq!(get_parent_locale(&pak), Some("es"));
    }

    #[test]
    fn parse_old_format_12b_header() {
        let data = {
            let mut buf = Vec::new();
            buf.extend_from_slice(PAK_MAGIC);
            buf.extend_from_slice(&PAK_VERSION.to_be_bytes());
            buf.extend_from_slice(&8u32.to_be_bytes()); // payload length
            buf.extend_from_slice(b"old-data");
            buf.extend_from_slice(&[0u8; 64]); // dummy sig
            buf
        };
        let (_msg, payload, _sig, parent) = parse_signed(&data).unwrap();
        assert_eq!(payload, b"old-data");
        assert_eq!(parent, None);
        assert_eq!(get_parent_locale(&data), None);
    }

    #[test]
    fn rejects_unsupported_version() {
        let mut bad = build_unsigned(b"x", None);
        bad[4..8].copy_from_slice(&99u32.to_be_bytes());
        assert!(parse_signed(&bad).is_err());
    }
}
