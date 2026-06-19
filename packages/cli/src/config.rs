use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub project: String,
    pub source_dir: String,
    pub output_dir: String,
    pub key_env: String,
    pub fallback: String,
    pub targets: Vec<Target>,
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
        anyhow::bail!("l10n4x.config.json not found in the current directory. Run 'l10n4x init' to create one.");
    }
    let data = fs::read_to_string(path)?;
    let config: Config = serde_json::from_str(&data)?;
    Ok(config)
}

pub fn get_encryption_key(config: &Config) -> Result<Vec<u8>, anyhow::Error> {
    let key_var = &config.key_env;
    let key_str = std::env::var(key_var)
        .map_err(|_| anyhow::anyhow!("Encryption key env variable '{}' is not set.", key_var))?;
    let key_bytes = key_str.as_bytes();
    if key_bytes.len() != 32 {
        anyhow::bail!(
            "Encryption key in environment variable '{}' must be exactly 32 bytes (got {} bytes).",
            key_var,
            key_bytes.len()
        );
    }
    Ok(key_bytes.to_vec())
}
