//! Structured error types for `l10n4x-core`.

/// Result alias for fallible core operations.
pub type CoreResult<T> = Result<T, CoreError>;

/// Structured errors emitted by `l10n4x-core` operations.
///
/// All variants carry a `&'static str` message describing the specific failure,
/// except `UnsupportedVersion` which carries the encountered version number,
/// and `EncodingError` which is a unit variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreError {
    /// Invalid or corrupt binary format.
    InvalidFormat(&'static str),
    /// Buffer too short, truncated data.
    BufferTooShort(&'static str),
    /// Invalid magic bytes.
    InvalidMagic(&'static str),
    /// Unsupported version number.
    UnsupportedVersion(u32),
    /// Signature verification failed.
    SignatureInvalid(&'static str),
    /// Key not configured (verify key, decrypt key, etc).
    KeyNotConfigured(&'static str),
    /// General I/O or decompression error.
    IoError(&'static str),
    /// Feature not enabled (e.g. encryption, alloc).
    FeatureNotEnabled(&'static str),
    /// UTF-8 encoding error.
    EncodingError,
    /// Integer overflow when computing lengths or offsets.
    Overflow(&'static str),
    /// Trailing data after expected end.
    TrailingData(&'static str),
}

#[cfg(feature = "std")]
impl std::error::Error for CoreError {}

impl core::fmt::Display for CoreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            CoreError::InvalidFormat(msg) => f.write_str(msg),
            CoreError::BufferTooShort(msg) => f.write_str(msg),
            CoreError::InvalidMagic(msg) => f.write_str(msg),
            CoreError::UnsupportedVersion(v) => write!(f, "Unsupported version {}", v),
            CoreError::SignatureInvalid(msg) => f.write_str(msg),
            CoreError::KeyNotConfigured(msg) => f.write_str(msg),
            CoreError::IoError(msg) => f.write_str(msg),
            CoreError::FeatureNotEnabled(msg) => f.write_str(msg),
            CoreError::EncodingError => f.write_str("UTF-8 encoding error"),
            CoreError::Overflow(msg) => f.write_str(msg),
            CoreError::TrailingData(msg) => f.write_str(msg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn display_invalid_format() {
        let e = CoreError::InvalidFormat("bad magic");
        assert_eq!(format!("{}", e), "bad magic");
    }

    #[test]
    fn display_buffer_too_short() {
        let e = CoreError::BufferTooShort("truncated header");
        assert_eq!(format!("{}", e), "truncated header");
    }

    #[test]
    fn display_invalid_magic() {
        let e = CoreError::InvalidMagic("bad bytes");
        assert_eq!(format!("{}", e), "bad bytes");
    }

    #[test]
    fn display_unsupported_version() {
        let e = CoreError::UnsupportedVersion(42);
        assert_eq!(format!("{}", e), "Unsupported version 42");
    }

    #[test]
    fn display_signature_invalid() {
        let e = CoreError::SignatureInvalid("verify failed");
        assert_eq!(format!("{}", e), "verify failed");
    }

    #[test]
    fn display_key_not_configured() {
        let e = CoreError::KeyNotConfigured("no key set");
        assert_eq!(format!("{}", e), "no key set");
    }

    #[test]
    fn display_io_error() {
        let e = CoreError::IoError("io failure");
        assert_eq!(format!("{}", e), "io failure");
    }

    #[test]
    fn display_feature_not_enabled() {
        let e = CoreError::FeatureNotEnabled("encryption");
        assert_eq!(format!("{}", e), "encryption");
    }

    #[test]
    fn display_encoding_error() {
        let e = CoreError::EncodingError;
        assert_eq!(format!("{}", e), "UTF-8 encoding error");
    }

    #[test]
    fn display_overflow() {
        let e = CoreError::Overflow("integer overflow");
        assert_eq!(format!("{}", e), "integer overflow");
    }

    #[test]
    fn display_trailing_data() {
        let e = CoreError::TrailingData("extra bytes");
        assert_eq!(format!("{}", e), "extra bytes");
    }

    #[test]
    fn debug_and_clone() {
        let e = CoreError::InvalidFormat("err");
        let _ = format!("{:?}", e);
        let _ = e;
    }

    #[test]
    fn partial_eq() {
        assert_eq!(CoreError::EncodingError, CoreError::EncodingError);
        assert_ne!(CoreError::EncodingError, CoreError::Overflow("x"));
    }
}
