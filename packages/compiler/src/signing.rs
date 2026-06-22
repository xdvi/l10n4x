use crate::CompileError;
use ed25519_dalek::{Signer, SigningKey};
use std::sync::OnceLock;

static SIGNING_KEY: OnceLock<[u8; 32]> = OnceLock::new();

/// Installs the 32-byte Ed25519 signing seed.
/// Enforces set-once constraint. Returns true if the key was successfully set.
pub fn set_signing_key(seed: &[u8]) -> bool {
    if seed.len() != 32 {
        return false;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(seed);
    SIGNING_KEY.set(arr).is_ok()
}

/// Derives the public key from the configured signing seed.
pub fn signing_public_key() -> Result<[u8; 32], CompileError> {
    let seed = SIGNING_KEY
        .get()
        .ok_or(CompileError::SigningKeyNotConfigured)?;
    let signing_key = SigningKey::from_bytes(seed);
    Ok(signing_key.verifying_key().to_bytes())
}

/// Signs a message with the configured signing seed.
pub fn sign(message: &[u8]) -> Result<[u8; 64], CompileError> {
    let seed = SIGNING_KEY
        .get()
        .ok_or(CompileError::SigningKeyNotConfigured)?;
    let signing_key = SigningKey::from_bytes(seed);
    let signature = signing_key.sign(message);
    Ok(signature.to_bytes())
}
