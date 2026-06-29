use crate::SyncContext;
use anyhow::Context;
use l10n4x_compiler::flatten_value;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub const TMS_FORMAT: &str = "l10n4x-tms";
pub const TMS_VERSION: u32 = 1;

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

pub fn export_file_bundle(ctx: &SyncContext, out_dir: &Path) -> Result<(), anyhow::Error> {
    fs::create_dir_all(out_dir)?;
    let bundle = scan_source_bundle(ctx)?;
    let path = out_dir.join("l10n4x-tms.json");
    let json = serde_json::to_string_pretty(&bundle)?;
    fs::write(path, json)?;
    Ok(())
}

pub fn import_file_bundle(ctx: &SyncContext, from_dir: &Path) -> Result<(), anyhow::Error> {
    let path = from_dir.join("l10n4x-tms.json");
    let raw = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let bundle: TmsBundle = serde_json::from_str(&raw)?;
    if bundle.format != TMS_FORMAT {
        anyhow::bail!("Unsupported TMS format '{}'", bundle.format);
    }
    write_bundle_to_source(ctx, &bundle)?;
    Ok(())
}

pub fn scan_source_bundle(ctx: &SyncContext) -> Result<TmsBundle, anyhow::Error> {
    let src = Path::new(&ctx.source_dir);
    if !src.is_dir() {
        anyhow::bail!("sourceDir '{}' is not a directory", ctx.source_dir);
    }

    let mut namespaces: HashMap<String, HashMap<String, HashMap<String, String>>> = HashMap::new();

    for locale_entry in fs::read_dir(src)? {
        let locale_entry = locale_entry?;
        let locale_path = locale_entry.path();
        if !locale_path.is_dir() {
            continue;
        }
        let locale = locale_entry.file_name().to_string_lossy().to_string();

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
        project: ctx.project.clone(),
        fallback: ctx.fallback.clone(),
        exported_at: iso_timestamp(),
        namespaces,
    })
}

pub fn write_bundle_to_source(ctx: &SyncContext, bundle: &TmsBundle) -> Result<(), anyhow::Error> {
    for (namespace, locales) in &bundle.namespaces {
        for (locale, flat) in locales {
            let nested = unflatten_keys(flat);
            let locale_dir = Path::new(&ctx.source_dir).join(locale);
            fs::create_dir_all(&locale_dir)?;
            let file_path = locale_dir.join(format!("{namespace}.json"));
            let json = serde_json::to_string_pretty(&nested)?;
            fs::write(file_path, json)?;
        }
    }
    Ok(())
}

pub fn unflatten_keys(flat: &HashMap<String, String>) -> serde_json::Value {
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

fn iso_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
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
}
