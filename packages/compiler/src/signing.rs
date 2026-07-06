use crate::CompileError;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use std::sync::OnceLock;

/// Cached signing material: the derived `SigningKey` and its precomputed `VerifyingKey`.
///
/// `ed25519-dalek` recomputes the SHA-512 seed expansion on every `sign()` internally
/// (its `ExpandedSecretKey` is not part of the 2.x public API), but we can still avoid
/// the per-call `SigningKey::from_bytes` and — crucially — the curve multiplication that
/// `verifying_key()` performs. Deriving both once at install time lets every `sign()` and
/// `signing_public_key()` reuse the cached values.
struct CachedKey {
    signing: SigningKey,
    verifying: VerifyingKey,
}

static SIGNING_KEY: OnceLock<CachedKey> = OnceLock::new();

/// Installs the 32-byte Ed25519 signing seed.
/// Enforces set-once constraint. Returns true if the key was successfully set.
pub fn set_signing_key(seed: &[u8]) -> bool {
    if seed.len() != 32 {
        return false;
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(seed);
    let signing = SigningKey::from_bytes(&arr);
    let verifying = signing.verifying_key();
    SIGNING_KEY.set(CachedKey { signing, verifying }).is_ok()
}

/// Derives the public key from the configured signing seed.
pub fn signing_public_key() -> Result<[u8; 32], CompileError> {
    Ok(SIGNING_KEY
        .get()
        .ok_or(CompileError::SigningKeyNotConfigured)?
        .verifying
        .to_bytes())
}

/// Signs a message with the configured signing seed.
pub fn sign(message: &[u8]) -> Result<[u8; 64], CompileError> {
    let cached = SIGNING_KEY
        .get()
        .ok_or(CompileError::SigningKeyNotConfigured)?;
    let signature = cached.signing.sign(message);
    Ok(signature.to_bytes())
}
