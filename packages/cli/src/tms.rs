//! TMS (Translation Management System) exchange: export/import locale JSON and push signed paks.

use crate::config::Config;
use anyhow::Context;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use l10n4x_compiler::flatten_value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

const TMS_FORMAT: &str = "l10n4x-tms";
const TMS_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncDirection {
    Export,
    Import,
    Push,
}

impl SyncDirection {
    pub fn parse(s: &str) -> Result<Self, anyhow::Error> {
        match s {
            "export" => Ok(Self::Export),
            "import" => Ok(Self::Import),
            "push" => Ok(Self::Push),
            other => anyhow::bail!(
                "Unknown sync direction '{}'. Use: export, import, push.",
                other
            ),
        }
    }
}

/// Portable exchange document for TMS tools and offline handoff.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TmsBundle {
    pub format: String,
    pub version: u32,
    pub project: String,
    pub fallback: String,
    pub exported_at: String,
    /// namespace → locale → flat dot-keys
    pub namespaces: HashMap<String, HashMap<String, HashMap<String, String>>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookPayload {
    project: String,
    pushed_at: String,
    bundle_mode: String,
    artifacts: Vec<WebhookArtifact>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WebhookArtifact {
    locale: String,
    namespace: Option<String>,
    path: String,
    sha256: String,
    pak_base64: String,
}

pub fn run_sync(
    config: &Config,
    provider: &str,
    direction: SyncDirection,
    out: Option<&str>,
    from: Option<&str>,
) -> Result<(), anyhow::Error> {
    match provider {
        "file" => match direction {
            SyncDirection::Export => {
                let dir = out.unwrap_or("tms-export");
                export_file_bundle(config, Path::new(dir))?;
                println!("TMS export written to '{}'", dir);
            }
            SyncDirection::Import => {
                let dir = from.ok_or_else(|| {
                    anyhow::anyhow!("--from is required for sync import")
                })?;
                import_file_bundle(config, Path::new(dir))?;
                println!("TMS import merged into '{}'", config.source_dir);
            }
            SyncDirection::Push => {
                anyhow::bail!("file provider does not support push; use --provider webhook");
            }
        },
        "webhook" => {
            if direction != SyncDirection::Push {
                anyhow::bail!("webhook provider only supports --direction push");
            }
            push_webhook(config)?;
            println!("Signed paks pushed to webhook");
        }
        "crowdin" => match direction {
            SyncDirection::Export => {
                let dir = out.unwrap_or("tms-crowdin");
                export_crowdin_tree(config, Path::new(dir))?;
                println!(
                    "Crowdin-compatible tree written to '{}' (upload via Crowdin UI or API)",
                    dir
                );
            }
            SyncDirection::Import => {
                let dir = from.ok_or_else(|| {
                    anyhow::anyhow!("--from is required for crowdin import")
                })?;
                import_crowdin_tree(config, Path::new(dir))?;
                println!("Crowdin tree imported into '{}'", config.source_dir);
            }
            SyncDirection::Push => {
                anyhow::bail!(
                    "crowdin push requires Crowdin API credentials; use export + Crowdin CLI, or --provider webhook"
                );
            }
        },
        other => anyhow::bail!(
            "Unknown TMS provider '{}'. Supported: file, webhook, crowdin.",
            other
        ),
    }
    Ok(())
}

pub fn maybe_push_webhook_after_build(config: &Config) -> Result<(), anyhow::Error> {
    let tms = match &config.tms {
        Some(t) if t.push_on_build && t.provider == "webhook" => t,
        _ => return Ok(()),
    };
    if tms.webhook_url.as_deref().unwrap_or("").is_empty() {
        eprintln!("Warning: tms.pushOnBuild is true but webhookUrl is empty — skipping push.");
        return Ok(());
    }
    push_webhook_with_config(config, tms)?;
    println!("Post-build TMS webhook push completed.");
    Ok(())
}

fn export_file_bundle(config: &Config, out_dir: &Path) -> Result<(), anyhow::Error> {
    fs::create_dir_all(out_dir)?;
    let bundle = scan_source_bundle(config)?;
    let path = out_dir.join("l10n4x-tms.json");
    let json = serde_json::to_string_pretty(&bundle)?;
    fs::write(&path, json)?;
    Ok(())
}

fn import_file_bundle(config: &Config, from_dir: &Path) -> Result<(), anyhow::Error> {
    let path = from_dir.join("l10n4x-tms.json");
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))?;
    let bundle: TmsBundle = serde_json::from_str(&raw)?;
    if bundle.format != TMS_FORMAT {
        anyhow::bail!("Unsupported TMS format '{}'", bundle.format);
    }
    write_bundle_to_source(config, &bundle)?;
    Ok(())
}

fn export_crowdin_tree(config: &Config, out_dir: &Path) -> Result<(), anyhow::Error> {
    let bundle = scan_source_bundle(config)?;
    for (namespace, locales) in &bundle.namespaces {
        for (locale, flat) in locales {
            let nested = unflatten_keys(flat);
            let locale_dir = out_dir.join(locale);
            fs::create_dir_all(&locale_dir)?;
            let file_path = locale_dir.join(format!("{namespace}.json"));
            let json = serde_json::to_string_pretty(&nested)?;
            fs::write(file_path, json)?;
        }
    }
    let readme = out_dir.join("README.txt");
    fs::write(
        readme,
        "Crowdin-compatible export from l10n4x.\n\
         Upload each locale/*.json file to your Crowdin project.\n\
         After translation, download and run:\n\
           l10n4x sync --provider crowdin --direction import --from <download-dir>\n",
    )?;
    Ok(())
}

fn import_crowdin_tree(config: &Config, from_dir: &Path) -> Result<(), anyhow::Error> {
    if !from_dir.is_dir() {
        anyhow::bail!("'{}' is not a directory", from_dir.display());
    }
    let mut namespaces: HashMap<String, HashMap<String, HashMap<String, String>>> =
        HashMap::new();

    for locale_entry in fs::read_dir(from_dir)? {
        let locale_entry = locale_entry?;
        let locale_path = locale_entry.path();
        if !locale_path.is_dir() {
            continue;
        }
        let locale = locale_entry
            .file_name()
            .to_string_lossy()
            .to_string();
        for file_entry in fs::read_dir(&locale_path)? {
            let file_entry = file_entry?;
            let file_path = file_entry.path();
            if file_path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let namespace = file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("invalid file name {}", file_path.display()))?
                .to_string();
            let content = fs::read_to_string(&file_path)?;
            let parsed: serde_json::Value = serde_json::from_str(&content)?;
            let mut flat = HashMap::new();
            flatten_value(namespace.clone(), &parsed, &mut flat);
            namespaces
                .entry(namespace)
                .or_default()
                .insert(locale.clone(), flat);
        }
    }

    let bundle = TmsBundle {
        format: TMS_FORMAT.to_string(),
        version: TMS_VERSION,
        project: config.project.clone(),
        fallback: config.fallback.clone(),
        exported_at: iso_timestamp(),
        namespaces,
    };
    write_bundle_to_source(config, &bundle)?;
    Ok(())
}

fn scan_source_bundle(config: &Config) -> Result<TmsBundle, anyhow::Error> {
    let src = Path::new(&config.source_dir);
    if !src.is_dir() {
        anyhow::bail!("sourceDir '{}' is not a directory", config.source_dir);
    }

    let mut namespaces: HashMap<String, HashMap<String, HashMap<String, String>>> =
        HashMap::new();

    for locale_entry in fs::read_dir(src)? {
        let locale_entry = locale_entry?;
        let locale_path = locale_entry.path();
        if !locale_path.is_dir() {
            continue;
        }
        let locale = locale_entry
            .file_name()
            .to_string_lossy()
            .to_string();

        for file_entry in fs::read_dir(&locale_path)? {
            let file_entry = file_entry?;
            let file_path = file_entry.path();
            if file_path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let namespace = file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("invalid file name {}", file_path.display()))?
                .to_string();
            let content = fs::read_to_string(&file_path)?;
            let parsed: serde_json::Value = serde_json::from_str(&content)?;
            let mut flat = HashMap::new();
            flatten_value(namespace.clone(), &parsed, &mut flat);
            namespaces
                .entry(namespace)
                .or_default()
                .insert(locale.clone(), flat);
        }
    }

    Ok(TmsBundle {
        format: TMS_FORMAT.to_string(),
        version: TMS_VERSION,
        project: config.project.clone(),
        fallback: config.fallback.clone(),
        exported_at: iso_timestamp(),
        namespaces,
    })
}

fn write_bundle_to_source(config: &Config, bundle: &TmsBundle) -> Result<(), anyhow::Error> {
    for (namespace, locales) in &bundle.namespaces {
        for (locale, flat) in locales {
            let nested = unflatten_keys(flat);
            let locale_dir = Path::new(&config.source_dir).join(locale);
            fs::create_dir_all(&locale_dir)?;
            let file_path = locale_dir.join(format!("{namespace}.json"));
            let json = serde_json::to_string_pretty(&nested)?;
            fs::write(file_path, json)?;
        }
    }
    Ok(())
}

fn unflatten_keys(flat: &HashMap<String, String>) -> serde_json::Value {
    let mut root = serde_json::Map::new();
    let mut sorted: Vec<_> = flat.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));

    for (key, value) in sorted {
        let parts: Vec<&str> = key.split('.').collect();
        if parts.is_empty() {
            continue;
        }
        insert_nested(&mut root, &parts, serde_json::Value::String(value.clone()));
    }
    serde_json::Value::Object(root)
}

fn insert_nested(
    map: &mut serde_json::Map<String, serde_json::Value>,
    parts: &[&str],
    value: serde_json::Value,
) {
    if parts.len() == 1 {
        map.insert(parts[0].to_string(), value);
        return;
    }
    let entry = map
        .entry(parts[0].to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if let serde_json::Value::Object(ref mut child) = entry {
        insert_nested(child, &parts[1..], value);
    }
}

fn push_webhook(config: &Config) -> Result<(), anyhow::Error> {
    let tms = config
        .tms
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("tms section missing in l10n4x.config.json"))?;
    push_webhook_with_config(config, tms)
}

fn push_webhook_with_config(
    config: &Config,
    tms: &crate::config::TmsConfig,
) -> Result<(), anyhow::Error> {
    let url = tms
        .webhook_url
        .as_deref()
        .filter(|u| !u.is_empty())
        .ok_or_else(|| anyhow::anyhow!("tms.webhookUrl is required for webhook push"))?;

    let token = tms
        .webhook_token_env
        .as_deref()
        .and_then(|var| std::env::var(var).ok());

    let artifacts = collect_pak_artifacts(config)?;
    let payload = WebhookPayload {
        project: config.project.clone(),
        pushed_at: iso_timestamp(),
        bundle_mode: config.bundles.mode.clone(),
        artifacts,
    };

    let body = serde_json::to_string(&payload)?;
    let mut req = ureq::post(url).set("Content-Type", "application/json");
    if let Some(token) = token {
        req = req.set("Authorization", &format!("Bearer {}", token));
    }
    let response = req.send_string(&body)?;
    if !(200..300).contains(&response.status()) {
        anyhow::bail!("webhook returned HTTP {}", response.status());
    }
    Ok(())
}

fn collect_pak_artifacts(config: &Config) -> Result<Vec<WebhookArtifact>, anyhow::Error> {
    let out = Path::new(&config.output_dir);
    let mut artifacts = Vec::new();

    if config.bundles.mode == "modular" {
        for locale_entry in fs::read_dir(out)? {
            let locale_entry = locale_entry?;
            let locale_path = locale_entry.path();
            if !locale_path.is_dir() {
                continue;
            }
            let locale = locale_entry.file_name().to_string_lossy().to_string();
            for pak_entry in fs::read_dir(&locale_path)? {
                let pak_entry = pak_entry?;
                let pak_path = pak_entry.path();
                if pak_path.extension().and_then(|e| e.to_str()) != Some("pak") {
                    continue;
                }
                let namespace = pak_path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string);
                artifacts.push(pak_artifact(&pak_path, &locale, namespace)?);
            }
        }
    } else {
        for entry in fs::read_dir(out)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("pak") {
                continue;
            }
            let locale = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow::anyhow!("invalid pak name {}", path.display()))?
                .to_string();
            artifacts.push(pak_artifact(&path, &locale, None)?);
        }
    }

    if artifacts.is_empty() {
        anyhow::bail!("no .pak artifacts found in '{}' — run `l10n4x build` first", config.output_dir);
    }
    artifacts.sort_by(|a, b| (a.locale.as_str(), a.namespace.as_deref()).cmp(&(b.locale.as_str(), b.namespace.as_deref())));
    Ok(artifacts)
}

fn pak_artifact(
    path: &Path,
    locale: &str,
    namespace: Option<String>,
) -> Result<WebhookArtifact, anyhow::Error> {
    let bytes = fs::read(path)?;
    let sha256 = sha256_hex(&bytes);
    Ok(WebhookArtifact {
        locale: locale.to_string(),
        namespace,
        path: path.display().to_string(),
        sha256,
        pak_base64: B64.encode(bytes),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

fn iso_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unflatten_roundtrip() {
        let mut flat = HashMap::new();
        flat.insert("common.welcome".to_string(), "Hi".to_string());
        flat.insert("common.nav.home".to_string(), "Home".to_string());
        let nested = unflatten_keys(&flat);
        assert!(nested.get("common").is_some());
        let common = nested.get("common").unwrap();
        assert_eq!(common.get("welcome").and_then(|v| v.as_str()), Some("Hi"));
    }

    #[test]
    fn sync_direction_parse() {
        assert_eq!(SyncDirection::parse("export").unwrap(), SyncDirection::Export);
        assert!(SyncDirection::parse("nope").is_err());
    }
}