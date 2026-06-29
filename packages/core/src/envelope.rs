//! Optional `L10E` outer envelope: AES-GCM wrapper around a signed `L10P` pak.

extern crate alloc;
use alloc::vec::Vec;

#[cfg(feature = "encryption")]
use crate::encryption;
use crate::error::CoreResult;

/// Magic bytes identifying an optional `L10E` encrypted outer envelope.
pub const ENVELOPE_MAGIC: &[u8; 4] = b"L10E";
/// Current `L10E` envelope format version.
pub const ENVELOPE_VERSION: u32 = 1;
/// Header size: magic + version + blob length.
pub const ENVELOPE_HEADER_SIZE: usize = 12;

/// Wraps a signed `L10P` pak in an encrypted `L10E` envelope.
pub fn wrap_encrypted(signed_pak: &[u8]) -> CoreResult<Vec<u8>> {
    #[cfg(not(feature = "encryption"))]
    {
        let _ = signed_pak;
        return Err(crate::CoreError::FeatureNotEnabled(
            "Encryption support not enabled",
        ));
    }
    #[cfg(feature = "encryption")]
    {
        let encrypted = encryption::encrypt_aes_gcm(signed_pak)?;
        let mut out = Vec::with_capacity(ENVELOPE_HEADER_SIZE + encrypted.len());
        out.extend_from_slice(ENVELOPE_MAGIC);
        out.extend_from_slice(&ENVELOPE_VERSION.to_be_bytes());
        out.extend_from_slice(&(encrypted.len() as u32).to_be_bytes());
        out.extend_from_slice(&encrypted);
        Ok(out)
    }
}

/// Unwraps an `L10E` envelope into the inner signed `L10P` bytes.
pub fn unwrap_encrypted(data: &[u8]) -> CoreResult<Vec<u8>> {
    if data.len() < ENVELOPE_HEADER_SIZE {
        return Err(crate::CoreError::BufferTooShort("Encrypted pak too short"));
    }
    if &data[0..4] != ENVELOPE_MAGIC {
        return Err(crate::CoreError::InvalidMagic(
            "Invalid encrypted pak magic",
        ));
    }
    let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
    if version != ENVELOPE_VERSION {
        return Err(crate::CoreError::UnsupportedVersion(version));
    }
    let blob_len = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
    let end = ENVELOPE_HEADER_SIZE
        .checked_add(blob_len)
        .ok_or(crate::CoreError::Overflow("Encrypted pak length overflow"))?;
    if data.len() != end {
        return Err(crate::CoreError::BufferTooShort("Encrypted pak truncated"));
    }
    #[cfg(not(feature = "encryption"))]
    {
        let _ = (&data, end);
        return Err(crate::CoreError::FeatureNotEnabled(
            "Encryption support not enabled",
        ));
    }
    #[cfg(feature = "encryption")]
    encryption::decrypt_aes_gcm(&data[ENVELOPE_HEADER_SIZE..end])
}

/// Opens on-disk bytes: returns inner signed `L10P` pak (decrypting `L10E` when needed).
pub fn open_outer(data: &[u8]) -> CoreResult<Vec<u8>> {
    if data.len() < 4 {
        return Err(crate::CoreError::BufferTooShort("Pak file too short"));
    }
    match &data[0..4] {
        b"L10P" => Ok(data.to_vec()),
        b"L10E" => unwrap_encrypted(data),
        _ => Err(crate::CoreError::InvalidFormat("Unknown pak format")),
    }
}

#[cfg(test)]
mod open_outer_tests {
    use super::*;

    #[test]
    fn open_outer_passes_through_l10p() {
        let data = b"L10P\x00\x00\x00\x01\x00\x00\x00\x00";
        let result = open_outer(data);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), data);
    }

    #[test]
    fn open_outer_rejects_short_data() {
        let result = open_outer(b"L10");
        assert!(result.is_err());
    }

    #[test]
    fn open_outer_rejects_unknown_magic() {
        let result = open_outer(b"XXXXsome data here");
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod unwrap_tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn unwrap_encrypted_too_short() {
        let result = unwrap_encrypted(b"L10E");
        assert!(result.is_err());
    }

    #[test]
    fn unwrap_encrypted_bad_magic() {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(b"XXXX");
        let result = unwrap_encrypted(&data);
        assert!(result.is_err());
    }

    #[test]
    fn unwrap_encrypted_wrong_version() {
        let mut data = vec![0u8; 12];
        data[0..4].copy_from_slice(b"L10E");
        data[4..8].copy_from_slice(&99u32.to_be_bytes());
        let result = unwrap_encrypted(&data);
        assert!(result.is_err());
    }

    #[test]
    fn unwrap_encrypted_truncated() {
        let mut data = vec![0u8; 14];
        data[0..4].copy_from_slice(b"L10E");
        data[4..8].copy_from_slice(&1u32.to_be_bytes());
        data[8..12].copy_from_slice(&100u32.to_be_bytes());
        let result = unwrap_encrypted(&data);
        assert!(result.is_err());
    }
}

#[cfg(all(test, feature = "encryption"))]
mod tests {
    use super::*;
    use crate::encryption::test_helpers::ENC_TEST_LOCK;
    use crate::pak::{build_unsigned, seal};

    #[test]
    fn l10e_roundtrip() {
        let _lock = ENC_TEST_LOCK.lock().unwrap();
        use crate::encryption;
        let enc_key = [9u8; 32];
        assert!(encryption::set_decrypt_key(&enc_key));

        let body = build_unsigned(b"payload", None);
        let sig = [0u8; 64];
        let signed = seal(&body, &sig);
        let wrapped = wrap_encrypted(&signed).unwrap();
        let opened = unwrap_encrypted(&wrapped).unwrap();
        assert_eq!(opened, signed);
    }
}
