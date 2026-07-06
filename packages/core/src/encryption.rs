//! Optional AES-256-GCM encryption for `.pak` transport/storage (`L10E` envelope).
//!
//! Signing (Ed25519) is always applied to the inner `L10P` container. Encryption is an
//! opt-in outer wrapper and does **not** replace signature verification.

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::error::CoreResult;

#[cfg(feature = "encryption")]
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};

const KEY_LEN: usize = 32;

/// Single atomically-swapped key slot. The previous scheme stored the key as
/// 8 independent `AtomicU32`s, so a reader racing a concurrent
/// `set_decrypt_key` could observe a torn key (half old, half new) and fail
/// to decrypt. Same epoch-reclaimed pattern as `integrity::VERIFY_KEY`.
static DECRYPT_KEY: AtomicPtr<[u8; KEY_LEN]> = AtomicPtr::new(core::ptr::null_mut());

fn load_key() -> Option<[u8; KEY_LEN]> {
    #[cfg(feature = "std")]
    let _guard = crossbeam_epoch::pin();
    let ptr = DECRYPT_KEY.load(Ordering::Acquire);
    if ptr.is_null() {
        return None;
    }
    // SAFETY: under std the epoch guard defers reclamation of the old slot;
    // under no_std execution is single-threaded.
    Some(unsafe { *ptr })
}

/// Sets the 32-byte AES key for optional `L10E` envelope encryption/decryption.
pub fn set_decrypt_key(key: &[u8]) -> bool {
    if key.len() != KEY_LEN {
        return false;
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(key);
    let new_ptr = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(arr));
    let old_ptr = DECRYPT_KEY.swap(new_ptr, Ordering::SeqCst);
    if !old_ptr.is_null() {
        crate::reclaim::schedule_drop(old_ptr);
    }
    true
}

/// Returns whether a decrypt key has been configured.
pub fn decrypt_key_configured() -> bool {
    !DECRYPT_KEY.load(Ordering::Acquire).is_null()
}

/// Encrypts bytes with AES-256-GCM (12-byte random nonce prepended to ciphertext).
#[cfg(feature = "encryption")]
pub fn encrypt_aes_gcm(plaintext: &[u8]) -> CoreResult<Vec<u8>> {
    let Some(key_bytes) = load_key() else {
        return Err(crate::CoreError::KeyNotConfigured(
            "Decrypt key not configured",
        ));
    };
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
        Err(crate::CoreError::FeatureNotEnabled(
            "Encryption requires std",
        ))
    }
}

/// Decrypts AES-256-GCM bytes (expects prepended 12-byte nonce).
#[cfg(feature = "encryption")]
pub fn decrypt_aes_gcm(data: &[u8]) -> CoreResult<Vec<u8>> {
    if data.len() < 12 {
        return Err(crate::CoreError::BufferTooShort(
            "Encrypted payload too short",
        ));
    }
    let Some(key_bytes) = load_key() else {
        return Err(crate::CoreError::KeyNotConfigured(
            "Decrypt key not configured",
        ));
    };
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
    Err(crate::CoreError::FeatureNotEnabled(
        "Encryption support not enabled",
    ))
}

#[cfg(not(feature = "encryption"))]
pub fn decrypt_aes_gcm(_data: &[u8]) -> CoreResult<Vec<u8>> {
    Err(crate::CoreError::FeatureNotEnabled(
        "Encryption support not enabled",
    ))
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
