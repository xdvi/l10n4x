//! Ed25519 signing (build) and verification (runtime) for `.pak` integrity.

extern crate alloc;

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[cfg(feature = "integrity")]
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

const KEY_LEN: usize = 32;
const SIG_LEN: usize = 64;

static VERIFY_PARTS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static VERIFY_SET: AtomicBool = AtomicBool::new(false);

#[cfg(all(feature = "integrity", feature = "std"))]
static SIGNING_PARTS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
#[cfg(all(feature = "integrity", feature = "std"))]
static SIGNING_SET: AtomicBool = AtomicBool::new(false);

fn load_key(parts: &[AtomicU32; 8]) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    for i in 0..8 {
        let val = parts[i].load(Ordering::SeqCst);
        key[i * 4..(i + 1) * 4].copy_from_slice(&val.to_be_bytes());
    }
    key
}

fn store_key(parts: &[AtomicU32; 8], key: &[u8; KEY_LEN]) {
    for i in 0..8 {
        let val = u32::from_be_bytes(key[i * 4..(i + 1) * 4].try_into().unwrap());
        parts[i].store(val, Ordering::SeqCst);
    }
}

/// Installs the 32-byte Ed25519 **public** key used to verify `.pak` signatures at runtime.
pub fn set_verify_key(key: &[u8]) -> bool {
    if key.len() != KEY_LEN {
        return false;
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(key);
    #[cfg(feature = "integrity")]
    {
        if VerifyingKey::from_bytes(&arr).is_err() {
            return false;
        }
    }
    store_key(&VERIFY_PARTS, &arr);
    VERIFY_SET.store(true, Ordering::SeqCst);
    true
}

/// Returns `true` if a verify key has been configured.
pub fn verify_key_configured() -> bool {
    VERIFY_SET.load(Ordering::SeqCst)
}

/// Verifies an Ed25519 signature over `message`.
pub fn verify(message: &[u8], signature: &[u8]) -> Result<(), &'static str> {
    if !VERIFY_SET.load(Ordering::SeqCst) {
        return Err("Verify key not configured");
    }
    if signature.len() != SIG_LEN {
        return Err("Invalid signature length");
    }
    #[cfg(feature = "integrity")]
    {
        let key_bytes = load_key(&VERIFY_PARTS);
        let verifying_key =
            VerifyingKey::from_bytes(&key_bytes).map_err(|_| "Invalid verify key")?;
        let mut sig_bytes = [0u8; SIG_LEN];
        sig_bytes.copy_from_slice(signature);
        let sig = Signature::from_bytes(&sig_bytes);
        verifying_key
            .verify(message, &sig)
            .map_err(|_| "Pak signature invalid")
    }
    #[cfg(not(feature = "integrity"))]
    {
        let _ = message;
        Err("Integrity support not enabled")
    }
}

/// Installs the 32-byte Ed25519 **signing** seed (build-time only, requires `std`).
#[cfg(all(feature = "integrity", feature = "std"))]
pub fn set_signing_key(seed: &[u8]) -> bool {
    if seed.len() != KEY_LEN {
        return false;
    }
    let mut arr = [0u8; KEY_LEN];
    arr.copy_from_slice(seed);
    store_key(&SIGNING_PARTS, &arr);
    SIGNING_SET.store(true, Ordering::SeqCst);
    true
}

/// Derives the public key from the configured signing seed.
#[cfg(all(feature = "integrity", feature = "std"))]
pub fn signing_public_key() -> Result<[u8; KEY_LEN], &'static str> {
    if !SIGNING_SET.load(Ordering::SeqCst) {
        return Err("Signing key not configured");
    }
    let seed = load_key(&SIGNING_PARTS);
    let signing_key = SigningKey::from_bytes(&seed);
    Ok(signing_key.verifying_key().to_bytes())
}

/// Signs `message` with the configured signing seed.
#[cfg(all(feature = "integrity", feature = "std"))]
pub fn sign(message: &[u8]) -> Result<[u8; SIG_LEN], &'static str> {
    if !SIGNING_SET.load(Ordering::SeqCst) {
        return Err("Signing key not configured");
    }
    let seed = load_key(&SIGNING_PARTS);
    let signing_key = SigningKey::from_bytes(&seed);
    let signature = signing_key.sign(message);
    Ok(signature.to_bytes())
}

#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify_roundtrip() {
        let _lock = TEST_LOCK.lock().unwrap();
        let seed = [7u8; 32];
        assert!(set_signing_key(&seed));
        let pubkey = signing_public_key().unwrap();
        assert!(set_verify_key(&pubkey));

        let message = b"L10P-test-message";
        let sig = sign(message).unwrap();
        assert!(verify(message, &sig).is_ok());
        assert!(verify(b"tampered", &sig).is_err());
    }
}
