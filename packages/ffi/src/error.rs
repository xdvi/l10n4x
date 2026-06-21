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
/// One or more parameters are null or contain invalid UTF-8.
pub const L10N4C_INVALID_PARAMS: i32 = 4;
/// An internal runtime error occurred (should not happen in normal use).
pub const L10N4C_INTERNAL_ERROR: i32 = 5;
/// File or directory I/O failed. Check that the path exists and is readable.
pub const L10N4C_IO_ERROR: i32 = 7;
/// Ed25519 signature verification failed — the `.pak` file may have been tampered with.
/// Re-compile with `l10n4x build` using the correct signing key.
pub const L10N4C_SIGNATURE_INVALID: i32 = 8;
/// Ed25519 verify public key has not been configured. Call `l10n4c_set_verify_key`
/// before loading any `.pak` files.
pub const L10N4C_VERIFY_KEY_NOT_SET: i32 = 9;
/// AES decrypt key has not been configured. Required only for `L10E`-encrypted paks.
/// Call `l10n4c_set_decrypt_key` before loading encrypted `.pak` files.
pub const L10N4C_DECRYPT_KEY_NOT_SET: i32 = 11;
