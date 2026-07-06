//! Ed25519 verification (runtime) for `.pak` integrity.

extern crate alloc;

use core::sync::atomic::{AtomicPtr, Ordering};

use crate::error::CoreResult;

#[cfg(feature = "alloc")]
use ed25519_dalek::Verifier;

const KEY_LEN: usize = 32;
const SIG_LEN: usize = 64;

/// Verify-key slot: raw bytes plus the parsed `VerifyingKey`, decoded ONCE at
/// install time. `VerifyingKey::from_bytes` performs curve point decompression;
/// re-running it on every signature verification is wasted work.
pub(crate) struct VerifyKeySlot {
    #[cfg_attr(feature = "alloc", allow(dead_code))]
    bytes: [u8; KEY_LEN],
    #[cfg(feature = "alloc")]
    parsed: ed25519_dalek::VerifyingKey,
}

pub(crate) static VERIFY_KEY: AtomicPtr<VerifyKeySlot> = AtomicPtr::new(core::ptr::null_mut());

/// Installs the 32-byte Ed25519 **public** key used to verify `.pak` signatures at runtime.
pub fn set_verify_key(key: &[u8]) -> bool {
    if key.len() != KEY_LEN {
        return false;
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(key);
    #[cfg(feature = "alloc")]
    let parsed = match ed25519_dalek::VerifyingKey::from_bytes(&arr) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let slot = VerifyKeySlot {
        bytes: arr,
        #[cfg(feature = "alloc")]
        parsed,
    };
    let new_ptr = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(slot));
    let old_ptr = VERIFY_KEY.swap(new_ptr, Ordering::SeqCst);
    if !old_ptr.is_null() {
        crate::reclaim::schedule_drop(old_ptr);
    }
    true
}

/// Returns `true` if a verify key has been configured.
pub fn verify_key_configured() -> bool {
    !VERIFY_KEY.load(Ordering::Acquire).is_null()
}

/// Verifies an Ed25519 signature over `message`.
pub fn verify(message: &[u8], signature: &[u8]) -> CoreResult<()> {
    if signature.len() != SIG_LEN {
        return Err(crate::CoreError::SignatureInvalid(
            "Invalid signature length",
        ));
    }
    #[cfg(feature = "alloc")]
    {
        let verifying_key: ed25519_dalek::VerifyingKey = {
            #[cfg(feature = "std")]
            {
                let _guard = crossbeam_epoch::pin();
                let loaded_ptr = VERIFY_KEY.load(Ordering::Acquire);
                if loaded_ptr.is_null() {
                    return Err(crate::CoreError::KeyNotConfigured(
                        "Verify key not configured",
                    ));
                }
                // SAFETY: Under the active epoch guard, dereferencing the pointer is safe
                // because memory reclamation is deferred until after the guard is dropped.
                unsafe { (*loaded_ptr).parsed }
            }
            #[cfg(not(feature = "std"))]
            {
                let loaded_ptr = VERIFY_KEY.load(Ordering::Acquire);
                if loaded_ptr.is_null() {
                    return Err(crate::CoreError::KeyNotConfigured(
                        "Verify key not configured",
                    ));
                }
                // SAFETY: In single-threaded environments, no concurrent mutations occur,
                // making dereferencing safe.
                unsafe { (*loaded_ptr).parsed }
            }
        };

        let mut sig_bytes = [0u8; SIG_LEN];
        sig_bytes.copy_from_slice(signature);
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        verifying_key
            .verify(message, &sig)
            .map_err(|_| crate::CoreError::SignatureInvalid("Pak signature invalid"))
    }
    #[cfg(not(feature = "alloc"))]
    {
        let _ = message;
        let _ = signature;
        Err(crate::CoreError::FeatureNotEnabled(
            "Integrity support not enabled",
        ))
    }
}

#[cfg(all(test, feature = "alloc"))]
extern crate std;

#[cfg(all(test, feature = "alloc"))]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::*;

    #[cfg(feature = "alloc")]
    #[test]
    fn verify_precomputed_signature() {
        let _lock = TEST_LOCK.lock().unwrap();

        // Reset verify key first
        let old_ptr = VERIFY_KEY.swap(core::ptr::null_mut(), Ordering::SeqCst);
        if !old_ptr.is_null() {
            crate::reclaim::schedule_drop(old_ptr);
        }

        assert!(set_verify_key(&TEST_PUBLIC_KEY));
        assert!(verify(TEST_MESSAGE, &TEST_SIGNATURE).is_ok());
        assert!(verify(b"tampered", &TEST_SIGNATURE).is_err());
    }

    #[test]
    fn set_verify_key_wrong_length() {
        assert!(!set_verify_key(&[0u8; 16]));
        assert!(!set_verify_key(&[0u8; 64]));
    }

    #[test]
    fn verify_key_configured_returns_false_initially() {
        let old = VERIFY_KEY.swap(core::ptr::null_mut(), Ordering::SeqCst);
        if !old.is_null() {
            crate::reclaim::schedule_drop(old);
        }
        assert!(!verify_key_configured());
    }

    #[test]
    fn verify_invalid_sig_length() {
        let old = VERIFY_KEY.swap(core::ptr::null_mut(), Ordering::SeqCst);
        if !old.is_null() {
            crate::reclaim::schedule_drop(old);
        }
        let key = [42u8; 32];
        assert!(set_verify_key(&key));
        let result = verify(b"msg", &[0u8; 32]);
        assert!(result.is_err());
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn verify_no_key_configured() {
        let old = VERIFY_KEY.swap(core::ptr::null_mut(), Ordering::SeqCst);
        if !old.is_null() {
            crate::reclaim::schedule_drop(old);
        }
        let result = verify(b"msg", &[0u8; 64]);
        assert!(result.is_err());
    }
}
