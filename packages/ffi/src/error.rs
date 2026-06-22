//! Structured error codes for the `l10n4c` C-FFI layer.
//!
//! These codes are returned by all `l10n4c_*` functions. Check the specific
//! function documentation for which codes it may return.

/// Operation completed successfully.
pub const L10N4C_OK: i32 = 0;
/// Translation key was not found in the requested or fallback locale.
pub const L10N4C_KEY_NOT_FOUND: i32 = 1;
/// The requested locale has not been loaded. Call `l10n4c_load_pak_directory` or
/// `l10n4c_load_pak_locale` first.
pub const L10N4C_LOCALE_NOT_LOADED: i32 = 2;
/// Caller-provided buffer is too small. Call the `_required_size` variant first,
/// allocate at least that many bytes, then retry.
pub const L10N4C_BUFFER_TOO_SMALL: i32 = 3;
/// One or more parameters are null.
pub const L10N4C_INVALID_PARAMS: i32 = 4;
/// An internal runtime error occurred (should not happen in normal use).
pub const L10N4C_INTERNAL_ERROR: i32 = 5;
/// Parameter contains invalid UTF-8 encoding.
pub const L10N4C_INVALID_ENCODING: i32 = 6;
/// File or directory I/O failed. Check that the path exists and is readable.
pub const L10N4C_IO_ERROR: i32 = 7;
/// Ed25519 signature verification failed — the `.pak` file may have been tampered with.
/// Re-compile with `l10n4x build` using the correct signing key.
pub const L10N4C_SIGNATURE_INVALID: i32 = 8;
/// Ed25519 verify public key has not been configured. Call `l10n4c_set_verify_key`
/// before loading any `.pak` files.
pub const L10N4C_VERIFY_KEY_NOT_SET: i32 = 9;
/// Library not initialized — call l10n4c_load_pak_directory or l10n4c_load_pak_locale first.
pub const L10N4C_NOT_INITIALIZED: i32 = 10;
/// AES decrypt key has not been configured. Required only for `L10E`-encrypted paks.
/// Call `l10n4c_set_decrypt_key` before loading encrypted `.pak` files.
pub const L10N4C_DECRYPT_KEY_NOT_SET: i32 = 11;
/// Operation resulted in an integer buffer overflow.
pub const L10N4C_BUFFER_OVERFLOW: i32 = 12;

// Compile-time static assertions to verify existing error codes are not modified.
const _: () = assert!(L10N4C_OK == 0);
const _: () = assert!(L10N4C_KEY_NOT_FOUND == 1);
const _: () = assert!(L10N4C_LOCALE_NOT_LOADED == 2);
const _: () = assert!(L10N4C_BUFFER_TOO_SMALL == 3);
const _: () = assert!(L10N4C_INVALID_PARAMS == 4);
const _: () = assert!(L10N4C_INTERNAL_ERROR == 5);
const _: () = assert!(L10N4C_INVALID_ENCODING == 6);
const _: () = assert!(L10N4C_IO_ERROR == 7);
const _: () = assert!(L10N4C_SIGNATURE_INVALID == 8);
const _: () = assert!(L10N4C_VERIFY_KEY_NOT_SET == 9);
const _: () = assert!(L10N4C_NOT_INITIALIZED == 10);
const _: () = assert!(L10N4C_DECRYPT_KEY_NOT_SET == 11);
const _: () = assert!(L10N4C_BUFFER_OVERFLOW == 12);

/// Maps a [`l10n4x_core::CoreError`] to the corresponding FFI status code.
pub fn core_error_to_ffi(err: l10n4x_core::CoreError) -> i32 {
    use l10n4x_core::CoreError;
    match err {
        CoreError::SignatureInvalid(_) => L10N4C_SIGNATURE_INVALID,
        CoreError::KeyNotConfigured(msg) if msg.contains("Verify") => L10N4C_VERIFY_KEY_NOT_SET,
        CoreError::KeyNotConfigured(msg) if msg.contains("Decrypt") => L10N4C_DECRYPT_KEY_NOT_SET,
        CoreError::KeyNotConfigured(_) => L10N4C_VERIFY_KEY_NOT_SET,
        CoreError::IoError(_) => L10N4C_IO_ERROR,
        CoreError::EncodingError => L10N4C_INVALID_ENCODING,
        CoreError::InvalidFormat(_)
        | CoreError::InvalidMagic(_)
        | CoreError::BufferTooShort(_)
        | CoreError::UnsupportedVersion(_)
        | CoreError::TrailingData(_) => L10N4C_SIGNATURE_INVALID,
        CoreError::Overflow(_) => L10N4C_BUFFER_OVERFLOW,
        CoreError::FeatureNotEnabled(_) => L10N4C_INTERNAL_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use l10n4x_core::CoreError;

    #[test]
    fn maps_signature_invalid() {
        assert_eq!(
            core_error_to_ffi(CoreError::SignatureInvalid("bad")),
            L10N4C_SIGNATURE_INVALID
        );
    }

    #[test]
    fn maps_verify_key_not_set() {
        assert_eq!(
            core_error_to_ffi(CoreError::KeyNotConfigured("Verify key not configured")),
            L10N4C_VERIFY_KEY_NOT_SET
        );
    }

    #[test]
    fn maps_decrypt_key_not_set() {
        assert_eq!(
            core_error_to_ffi(CoreError::KeyNotConfigured("Decrypt key not configured")),
            L10N4C_DECRYPT_KEY_NOT_SET
        );
    }
}
