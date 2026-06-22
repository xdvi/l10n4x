use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub project: String,
    pub source_dir: String,
    pub output_dir: String,
    pub fallback: String,
    /// Env var holding the 32-byte Ed25519 signing seed (build-time only).
    #[serde(default = "default_signing_key_env")]
    pub signing_key_env: String,
    /// Hex-encoded 32-byte Ed25519 public key embedded in client bindings.
    #[serde(default)]
    pub verify_public_key: Option<String>,
    /// When true, wraps signed `.pak` files in an optional `L10E` AES-GCM envelope.
    #[serde(default)]
    pub encrypt: bool,
    /// zstd compression level (1-22, default 8).
    #[serde(default = "default_compression_level")]
    pub compression_level: i32,
    /// Env var holding the 32-byte AES key (build + runtime, only when `encrypt` is true).
    #[serde(default = "default_encrypt_key_env")]
    pub encrypt_key_env: String,
    #[serde(default)]
    pub cors_origins: Option<Vec<String>>,
    pub targets: Vec<Target>,
}

fn default_encrypt_key_env() -> String {
    "L10N4X_ENCRYPT_KEY".to_string()
}

fn default_signing_key_env() -> String {
    "L10N4X_SIGNING_KEY".to_string()
}

fn default_compression_level() -> i32 {
    8
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub r#type: String,
    pub out_dir: String,
    #[serde(default)]
    pub options: serde_json::Value,
}

pub fn load_config() -> Result<Config, anyhow::Error> {
    let path = Path::new("l10n4x.config.json");
    if !path.exists() {
        anyhow::bail!(
            "l10n4x.config.json not found in the current directory.\n\
             Run `l10n4x init` to create one."
        );
    }
    let data = fs::read_to_string(path)?;
    let config: Config = serde_json::from_str(&data)?;
    Ok(config)
}

pub fn save_config(config: &Config) -> Result<(), anyhow::Error> {
    let path = Path::new("l10n4x.config.json");
    let content = serde_json::to_string_pretty(config)?;
    fs::write(path, content)?;
    Ok(())
}

/// Decodes a 32-byte secret key from one of three wire formats:
/// - 64 hex characters (lowercase or uppercase) — recommended for CI
/// - 44-char standard base64 (with `=` padding) or 43-char base64url (no padding)
/// - 32 raw bytes (backward-compatible, prints a deprecation warning)
pub fn decode_32_byte_key(raw: &str, var_name: &str) -> Result<[u8; 32], anyhow::Error> {
    let bytes = raw.as_bytes();

    // --- 64 hex chars ---
    if bytes.len() == 64 && bytes.iter().all(|b| b.is_ascii_hexdigit()) {
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).map_err(|_| {
                anyhow::anyhow!("{}: invalid hex digit at position {}", var_name, i * 2)
            })?;
        }
        return Ok(out);
    }

    // --- base64 standard (44 chars with =) or base64url no-pad (43 chars) ---
    if bytes.len() == 44 || bytes.len() == 43 {
        let decoded = general_purpose::STANDARD
            .decode(raw)
            .or_else(|_| general_purpose::URL_SAFE_NO_PAD.decode(raw))
            .map_err(|e| anyhow::anyhow!("{}: base64 decode failed: {}", var_name, e))?;
        if decoded.len() != 32 {
            anyhow::bail!(
                "{}: base64 decoded to {} bytes, expected 32.",
                var_name,
                decoded.len()
            );
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&decoded);
        return Ok(out);
    }

    // --- raw 32 bytes (backward-compat) ---
    if bytes.len() == 32 {
        eprintln!(
            "WARNING: {} looks like raw ASCII bytes. Prefer 64-char hex or base64 for security.",
            var_name
        );
        let mut out = [0u8; 32];
        out.copy_from_slice(bytes);
        return Ok(out);
    }

    anyhow::bail!(
        "{}: unsupported key format (got {} chars). Use 64-char hex or 44-char base64.",
        var_name,
        raw.len()
    )
}

/// Reads the 32-byte Ed25519 signing seed from the configured env var (build only).
pub fn get_signing_key(config: &Config) -> Result<[u8; 32], anyhow::Error> {
    let var = &config.signing_key_env;
    let raw =
        std::env::var(var).map_err(|_| anyhow::anyhow!("Signing key env '{}' is not set.", var))?;
    decode_32_byte_key(&raw, var)
}

pub fn parse_verify_public_key(hex: &str) -> Result<[u8; 32], anyhow::Error> {
    if hex.len() != 64 {
        anyhow::bail!("verifyPublicKey must be 64 hex characters (32 bytes).");
    }
    let mut key = [0u8; 32];
    for i in 0..32 {
        key[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| anyhow::anyhow!("Invalid hex in verifyPublicKey"))?;
    }
    Ok(key)
}

pub fn format_verify_public_key(key: &[u8; 32]) -> String {
    key.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Reads the 32-byte AES decrypt key from the configured env var (build + runtime).
pub fn get_encrypt_key(config: &Config) -> Result<[u8; 32], anyhow::Error> {
    let var = &config.encrypt_key_env;
    let raw =
        std::env::var(var).map_err(|_| anyhow::anyhow!("Encrypt key env '{}' is not set.", var))?;
    decode_32_byte_key(&raw, var)
}

pub fn format_verify_key_bytes(hex: &str) -> Result<String, anyhow::Error> {
    let key = parse_verify_public_key(hex)?;
    Ok(key
        .iter()
        .map(|b| format!("0x{:02x}", b))
        .collect::<Vec<_>>()
        .join(", "))
}

#[cfg(test)]
mod verify_key_tests {
    use super::*;

    #[test]
    fn parse_verify_public_key_valid() {
        let hex = "ab".repeat(32);
        let key = parse_verify_public_key(&hex).unwrap();
        assert_eq!(key, [0xabu8; 32]);
    }

    #[test]
    fn parse_verify_public_key_wrong_length() {
        assert!(parse_verify_public_key("ab").is_err());
        assert!(parse_verify_public_key(&"a".repeat(63)).is_err());
    }

    #[test]
    fn parse_verify_public_key_invalid_hex() {
        assert!(parse_verify_public_key(&"zz".repeat(32)).is_err());
    }

    #[test]
    fn format_verify_public_key_roundtrip() {
        let key = [0xabu8; 32];
        let hex = format_verify_public_key(&key);
        assert_eq!(hex, "ab".repeat(32));
        let parsed = parse_verify_public_key(&hex).unwrap();
        assert_eq!(parsed, key);
    }

    #[test]
    fn format_verify_key_bytes_format() {
        let hex = "ab".repeat(32);
        let result = format_verify_key_bytes(&hex).unwrap();
        assert!(result.contains("0xab"));
        assert!(result.contains(", "));
    }

    #[test]
    fn format_verify_key_bytes_invalid_input() {
        assert!(format_verify_key_bytes("zz").is_err());
    }
}

#[cfg(test)]
mod key_encoding_tests {
    use super::*;

    fn call_decode(raw: &str) -> Result<[u8; 32], anyhow::Error> {
        decode_32_byte_key(raw, "TEST_VAR")
    }

    #[test]
    fn accepts_64_char_lowercase_hex() {
        let hex = "ab".repeat(32);
        let result = call_decode(&hex).unwrap();
        assert_eq!(result, [0xab_u8; 32]);
    }

    #[test]
    fn accepts_64_char_uppercase_hex() {
        let hex = "AB".repeat(32);
        let result = call_decode(&hex).unwrap();
        assert_eq!(result, [0xab_u8; 32]);
    }

    #[test]
    fn accepts_base64_standard_with_padding() {
        use base64::{engine::general_purpose, Engine as _};
        let encoded = general_purpose::STANDARD.encode([0xff_u8; 32]);
        assert_eq!(encoded.len(), 44);
        let result = call_decode(&encoded).unwrap();
        assert_eq!(result, [0xff_u8; 32]);
    }

    #[test]
    fn accepts_base64url_no_padding() {
        use base64::{engine::general_purpose, Engine as _};
        let encoded = general_purpose::URL_SAFE_NO_PAD.encode([0xaa_u8; 32]);
        assert_eq!(encoded.len(), 43);
        let result = call_decode(&encoded).unwrap();
        assert_eq!(result, [0xaa_u8; 32]);
    }

    #[test]
    fn accepts_raw_32_bytes() {
        let raw: String = (0u8..32).map(|b| b as char).collect();
        assert_eq!(raw.len(), 32);
        let result = call_decode(&raw).unwrap();
        let expected: [u8; 32] = core::array::from_fn(|i| i as u8);
        assert_eq!(result, expected);
    }

    #[test]
    fn rejects_wrong_length() {
        assert!(call_decode("tooshort").is_err());
        assert!(call_decode(&"x".repeat(33)).is_err());
        assert!(call_decode(&"a".repeat(63)).is_err());
    }

    #[test]
    fn rejects_invalid_hex() {
        let bad = "zz".repeat(32);
        assert!(call_decode(&bad).is_err());
    }
}

#[cfg(test)]
mod config_io_and_env_tests {
    use super::*;
    use std::env;

    #[test]
    fn test_load_save_config_temp() {
        let temp = tempfile::tempdir().unwrap();
        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp.path()).unwrap();

        // 1. load_config should fail because it doesn't exist
        assert!(load_config().is_err());

        // 2. save a valid config
        let cfg = Config {
            project: "test".to_string(),
            source_dir: "src".to_string(),
            output_dir: "out".to_string(),
            fallback: "en".to_string(),
            signing_key_env: "SIGN_KEY".to_string(),
            verify_public_key: None,
            encrypt: false,
            compression_level: 8,
            encrypt_key_env: "ENC_KEY".to_string(),
            cors_origins: None,
            targets: vec![],
        };
        save_config(&cfg).unwrap();

        // 3. load_config should now succeed and match
        let loaded = load_config().unwrap();
        assert_eq!(loaded.project, "test");

        env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    fn default_key_env_vars() {
        assert_eq!(default_encrypt_key_env(), "L10N4X_ENCRYPT_KEY");
        assert_eq!(default_signing_key_env(), "L10N4X_SIGNING_KEY");
    }

    #[test]
    fn decode_base64_wrong_decoded_length() {
        // base64 of 31 zero bytes produces 44 chars but decodes to 31 bytes
        let thirty_one_bytes = general_purpose::STANDARD.encode([0u8; 31]);
        assert_eq!(thirty_one_bytes.len(), 44);
        let result = decode_32_byte_key(&thirty_one_bytes, "test");
        assert!(result.is_err());
        let msg = format!("{}", result.err().unwrap());
        assert!(msg.contains("bytes, expected 32"), "msg: {}", msg);
    }

    #[test]
    fn test_get_keys_from_env() {
        let cfg = Config {
            project: "test".to_string(),
            source_dir: "src".to_string(),
            output_dir: "out".to_string(),
            fallback: "en".to_string(),
            signing_key_env: "TEST_SIGN_KEY_ENV".to_string(),
            verify_public_key: None,
            encrypt: false,
            compression_level: 8,
            encrypt_key_env: "TEST_ENC_KEY_ENV".to_string(),
            cors_origins: None,
            targets: vec![],
        };

        // 1. Env vars not set
        assert!(get_signing_key(&cfg).is_err());
        assert!(get_encrypt_key(&cfg).is_err());

        // 2. Set them
        let key_hex = "ab".repeat(32);
        env::set_var("TEST_SIGN_KEY_ENV", &key_hex);
        env::set_var("TEST_ENC_KEY_ENV", &key_hex);

        assert_eq!(get_signing_key(&cfg).unwrap(), [0xab; 32]);
        assert_eq!(get_encrypt_key(&cfg).unwrap(), [0xab; 32]);

        env::remove_var("TEST_SIGN_KEY_ENV");
        env::remove_var("TEST_ENC_KEY_ENV");
    }
}
