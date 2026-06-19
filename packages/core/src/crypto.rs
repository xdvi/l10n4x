extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce, Key
};

static KEY_PARTS: [AtomicU32; 8] = [
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
    AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0),
];
static KEY_SET: AtomicBool = AtomicBool::new(false);

/// Returns the currently configured 32-byte encryption key.
/// Returns an error if the key has not been set or is invalid.
pub fn get_encryption_key() -> Result<[u8; 32], &'static str> {
    if !KEY_SET.load(Ordering::SeqCst) {
        return Err("Encryption key has not been configured");
    }
    let mut key = [0u8; 32];
    for i in 0..8 {
        let val = KEY_PARTS[i].load(Ordering::SeqCst);
        key[i*4 .. (i+1)*4].copy_from_slice(&val.to_ne_bytes());
    }
    Ok(key)
}

/// Sets the global 32-byte AES key for GCM encryption and decryption.
pub fn set_encryption_key(key_slice: &[u8]) -> bool {
    if key_slice.len() != 32 {
        return false;
    }
    for i in 0..8 {
        let val = u32::from_ne_bytes(key_slice[i*4 .. (i+1)*4].try_into().unwrap());
        KEY_PARTS[i].store(val, Ordering::SeqCst);
    }
    KEY_SET.store(true, Ordering::SeqCst);
    true
}

/// Encrypts bytes using AES-256-GCM. Prepend 12-byte random nonce to ciphertext.
/// Note: OsRng requires `getrandom` feature which is active when standard features are enabled.
pub fn encrypt_gcm(plaintext: &[u8]) -> Result<Vec<u8>, &'static str> {
    let key_bytes = get_encryption_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    
    // In no_std target without getrandom, we can use a basic seed/nonce,
    // but standard build config provides getrandom. Let's make sure it compiles.
    #[cfg(feature = "std")]
    {
        use aes_gcm::aead::AeadCore;
        let nonce = Aes256Gcm::generate_nonce(&mut aes_gcm::aead::OsRng);
        let mut ciphertext = cipher.encrypt(&nonce, plaintext)
            .map_err(|_| "Encryption failed")?;
        let mut result = nonce.to_vec();
        result.append(&mut ciphertext);
        Ok(result)
    }
    #[cfg(not(feature = "std"))]
    {
        let _ = plaintext;
        let _ = cipher;
        // Fallback for strict no_std environments if std is not active
        // For security, strict no_std should ideally pass nonce from outside, but we provide a basic fallback or error.
        Err("Encryption is only supported when std feature is enabled")
    }
}

/// Decrypts bytes using AES-256-GCM. Expects prepended 12-byte nonce.
pub fn decrypt_gcm(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() < 12 {
        return Err("Data too short to contain nonce");
    }
    
    let key_bytes = get_encryption_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    
    let plaintext = cipher.decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption failed")?;
    
    Ok(plaintext)
}
