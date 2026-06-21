//! Ed25519 verification (runtime) for `.pak` integrity.

extern crate alloc;

use core::sync::atomic::{AtomicPtr, Ordering};

#[cfg(feature = "alloc")]
use ed25519_dalek::Verifier;

const KEY_LEN: usize = 32;
const SIG_LEN: usize = 64;

pub(crate) static VERIFY_KEY: AtomicPtr<[u8; 32]> = AtomicPtr::new(core::ptr::null_mut());

/// Installs the 32-byte Ed25519 **public** key used to verify `.pak` signatures at runtime.
pub fn set_verify_key(key: &[u8]) -> bool {
    if key.len() != KEY_LEN {
        return false;
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(key);
    #[cfg(feature = "alloc")]
    {
        if ed25519_dalek::VerifyingKey::from_bytes(&arr).is_err() {
            return false;
        }
    }
    let new_ptr = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(arr));
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
pub fn verify(message: &[u8], signature: &[u8]) -> Result<(), &'static str> {
    if signature.len() != SIG_LEN {
        return Err("Invalid signature length");
    }
    #[cfg(feature = "alloc")]
    {
        let key: [u8; 32] = {
            #[cfg(feature = "std")]
            {
                let _guard = crossbeam_epoch::pin();
                let loaded_ptr = VERIFY_KEY.load(Ordering::Acquire);
                if loaded_ptr.is_null() {
                    return Err("Verify key not configured");
                }
                // SAFETY: Under the active epoch guard, dereferencing the pointer is safe
                // because memory reclamation is deferred until after the guard is dropped.
                unsafe { *loaded_ptr }
            }
            #[cfg(not(feature = "std"))]
            {
                let loaded_ptr = VERIFY_KEY.load(Ordering::Acquire);
                if loaded_ptr.is_null() {
                    return Err("Verify key not configured");
                }
                // SAFETY: In single-threaded environments, no concurrent mutations occur,
                // making dereferencing safe.
                unsafe { *loaded_ptr }
            }
        };

        let verifying_key =
            ed25519_dalek::VerifyingKey::from_bytes(&key).map_err(|_| "Invalid verify key")?;
        let mut sig_bytes = [0u8; SIG_LEN];
        sig_bytes.copy_from_slice(signature);
        let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
        verifying_key
            .verify(message, &sig)
            .map_err(|_| "Pak signature invalid")
    }
    #[cfg(not(feature = "alloc"))]
    {
        let _ = message;
        let _ = signature;
        Err("Integrity support not enabled")
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
}
