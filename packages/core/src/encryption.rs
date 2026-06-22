//! Optional AES-256-GCM encryption for `.pak` transport/storage (`L10E` envelope).
//!
//! Signing (Ed25519) is always applied to the inner `L10P` container. Encryption is an
//! opt-in outer wrapper and does **not** replace signature verification.

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use crate::error::CoreResult;

#[cfg(feature = "encryption")]
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};

const KEY_LEN: usize = 32;

static KEY_PARTS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static KEY_SET: AtomicBool = AtomicBool::new(false);

fn load_key() -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    for i in 0..8 {
        let val = KEY_PARTS[i].load(Ordering::SeqCst);
        key[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
    }
    key
}

fn store_key(key: &[u8; KEY_LEN]) {
    for i in 0..8 {
        let val = u32::from_be_bytes(key[i * 4..(i + 1) * 4].try_into().unwrap());
        KEY_PARTS[i].store(val, Ordering::SeqCst);
    }
}

/// Sets the 32-byte AES key for optional `L10E` envelope encryption/decryption.
pub fn set_decrypt_key(key: &[u8]) -> bool {
    if key.len() != KEY_LEN {
        return false;
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(key);
    store_key(&arr);
    KEY_SET.store(true, Ordering::SeqCst);
    true
}

/// Returns whether a decrypt key has been configured.
pub fn decrypt_key_configured() -> bool {
    KEY_SET.load(Ordering::SeqCst)
}

/// Encrypts bytes with AES-256-GCM (12-byte random nonce prepended to ciphertext).
#[cfg(feature = "encryption")]
pub fn encrypt_aes_gcm(plaintext: &[u8]) -> CoreResult<Vec<u8>> {
    if !KEY_SET.load(Ordering::SeqCst) {
        return Err(crate::CoreError::KeyNotConfigured("Decrypt key not configured"));
    }
    let key_bytes = load_key();
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    #[cfg(feature = "std")]
    {
        use aes_gcm::aead::AeadCore;
        let nonce = Aes256Gcm::generate_nonce(&mut aes_gcm::aead::OsRng);
        let mut ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|_| crate::CoreError::IoError("Encryption failed"))?;
        let mut out = nonce.to_vec();
        out.append(&mut ciphertext);
        Ok(out)
    }
    #[cfg(not(feature = "std"))]
    {
        let _ = (cipher, plaintext);
        Err(crate::CoreError::FeatureNotEnabled("Encryption requires std"))
    }
}

/// Decrypts AES-256-GCM bytes (expects prepended 12-byte nonce).
#[cfg(feature = "encryption")]
pub fn decrypt_aes_gcm(data: &[u8]) -> CoreResult<Vec<u8>> {
    if data.len() < 12 {
        return Err(crate::CoreError::BufferTooShort("Encrypted payload too short"));
    }
    if !KEY_SET.load(Ordering::SeqCst) {
        return Err(crate::CoreError::KeyNotConfigured("Decrypt key not configured"));
    }
    let key_bytes = load_key();
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| crate::CoreError::SignatureInvalid("Decryption failed"))
}

#[cfg(not(feature = "encryption"))]
pub fn encrypt_aes_gcm(_plaintext: &[u8]) -> CoreResult<Vec<u8>> {
    Err(crate::CoreError::FeatureNotEnabled("Encryption support not enabled"))
}

#[cfg(not(feature = "encryption"))]
pub fn decrypt_aes_gcm(_data: &[u8]) -> CoreResult<Vec<u8>> {
    Err(crate::CoreError::FeatureNotEnabled("Encryption support not enabled"))
}

#[cfg(all(test, feature = "encryption"))]
pub(crate) mod test_helpers {
    use std::sync::Mutex;
    /// Serializes encryption tests that modify global key state.
    pub(crate) static ENC_TEST_LOCK: Mutex<()> = Mutex::new(());
}

#[cfg(all(test, feature = "encryption"))]
mod tests {
    use super::*;
    use crate::encryption::test_helpers::ENC_TEST_LOCK;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let _lock = ENC_TEST_LOCK.lock().unwrap();
        let key = [42u8; 32];
        assert!(set_decrypt_key(&key));
        let data = b"Hello l10n4x secret!";
        let encrypted = encrypt_aes_gcm(data).unwrap();
        let decrypted = decrypt_aes_gcm(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn decrypt_fails_when_payload_too_short() {
        let _lock = ENC_TEST_LOCK.lock().unwrap();
        let key = [0u8; 32];
        set_decrypt_key(&key);
        let result = decrypt_aes_gcm(b"too short");
        assert!(result.is_err());
    }

    #[test]
    fn decrypt_fails_without_key_set() {
        let _lock = ENC_TEST_LOCK.lock().unwrap();
        let key = [1u8; 32];
        set_decrypt_key(&key);
        let result = decrypt_aes_gcm(b"short");
        assert!(result.is_err());
    }

    #[test]
    fn set_decrypt_key_wrong_length_returns_false() {
        let result = set_decrypt_key(&[0u8; 16]);
        assert!(!result);
    }
}
