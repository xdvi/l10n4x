//! Optional `L10E` outer envelope: AES-GCM wrapper around a signed `L10P` pak.

extern crate alloc;
use alloc::vec::Vec;

#[cfg(feature = "encryption")]
use crate::encryption;

/// Magic bytes identifying an optional `L10E` encrypted outer envelope.
pub const ENVELOPE_MAGIC: &[u8; 4] = b"L10E";
/// Current `L10E` envelope format version.
pub const ENVELOPE_VERSION: u32 = 1;
/// Header size: magic + version + blob length.
pub const ENVELOPE_HEADER_SIZE: usize = 12;

/// Wraps a signed `L10P` pak in an encrypted `L10E` envelope.
pub fn wrap_encrypted(signed_pak: &[u8]) -> Result<Vec<u8>, &'static str> {
    #[cfg(not(feature = "encryption"))]
    {
        let _ = signed_pak;
        return Err("Encryption support not enabled");
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
pub fn unwrap_encrypted(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() < ENVELOPE_HEADER_SIZE {
        return Err("Encrypted pak too short");
    }
    if &data[0..4] != ENVELOPE_MAGIC {
        return Err("Invalid encrypted pak magic");
    }
    let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
    if version != ENVELOPE_VERSION {
        return Err("Unsupported encrypted pak version");
    }
    let blob_len = u32::from_be_bytes(data[8..12].try_into().unwrap()) as usize;
    let end = ENVELOPE_HEADER_SIZE
        .checked_add(blob_len)
        .ok_or("Encrypted pak length overflow")?;
    if data.len() != end {
        return Err("Encrypted pak truncated");
    }
    #[cfg(not(feature = "encryption"))]
    {
        let _ = (&data, end);
        return Err("Encryption support not enabled");
    }
    #[cfg(feature = "encryption")]
    encryption::decrypt_aes_gcm(&data[ENVELOPE_HEADER_SIZE..end])
}

/// Opens on-disk bytes: returns inner signed `L10P` pak (decrypting `L10E` when needed).
pub fn open_outer(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() < 4 {
        return Err("Pak file too short");
    }
    match &data[0..4] {
        b"L10P" => Ok(data.to_vec()),
        b"L10E" => unwrap_encrypted(data),
        _ => Err("Unknown pak format"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrity;
    use crate::pak::{build_unsigned, seal};

    #[test]
    #[cfg(feature = "encryption")]
    fn l10e_roundtrip() {
        use crate::encryption;
        let seed = [5u8; 32];
        assert!(integrity::set_signing_key(&seed));
        let enc_key = [9u8; 32];
        assert!(encryption::set_decrypt_key(&enc_key));

        let body = build_unsigned(b"payload");
        let sig = integrity::sign(&body).unwrap();
        let signed = seal(&body, &sig);
        let wrapped = wrap_encrypted(&signed).unwrap();
        let opened = unwrap_encrypted(&wrapped).unwrap();
        assert_eq!(opened, signed);
    }
}
