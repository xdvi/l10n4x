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
    /// Env var holding the 32-byte AES key (build + runtime, only when `encrypt` is true).
    #[serde(default = "default_encrypt_key_env")]
    pub encrypt_key_env: String,
    pub targets: Vec<Target>,
}

fn default_encrypt_key_env() -> String {
    "L10N4X_ENCRYPT_KEY".to_string()
}

fn default_signing_key_env() -> String {
    "L10N4X_SIGNING_KEY".to_string()
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

/// Reads the 32-byte Ed25519 signing seed from the configured env var (build only).
pub fn get_signing_key(config: &Config) -> Result<[u8; 32], anyhow::Error> {
    let var = &config.signing_key_env;
    let raw =
        std::env::var(var).map_err(|_| anyhow::anyhow!("Signing key env '{}' is not set.", var))?;
    let bytes = raw.as_bytes();
    if bytes.len() != 32 {
        anyhow::bail!(
            "Signing key in '{}' must be exactly 32 bytes (got {}).",
            var,
            bytes.len()
        );
    }
    let mut seed = [0u8; 32];
    seed.copy_from_slice(bytes);
    Ok(seed)
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

/// Formats a hex public key as comma-separated byte literals for generated source code.
/// Reads the 32-byte AES decrypt key from the configured env var (build + runtime).
pub fn get_encrypt_key(config: &Config) -> Result<[u8; 32], anyhow::Error> {
    let var = &config.encrypt_key_env;
    let raw =
        std::env::var(var).map_err(|_| anyhow::anyhow!("Encrypt key env '{}' is not set.", var))?;
    let bytes = raw.as_bytes();
    if bytes.len() != 32 {
        anyhow::bail!(
            "Encrypt key in '{}' must be exactly 32 bytes (got {}).",
            var,
            bytes.len()
        );
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(bytes);
    Ok(key)
}

pub fn format_verify_key_bytes(hex: &str) -> Result<String, anyhow::Error> {
    let key = parse_verify_public_key(hex)?;
    Ok(key
        .iter()
        .map(|b| format!("0x{:02x}", b))
        .collect::<Vec<_>>()
        .join(", "))
}
