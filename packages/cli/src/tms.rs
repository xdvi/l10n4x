//! TMS (Translation Management System) exchange: core providers + plugin dispatch.

use crate::config::Config;
use crate::plugins;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use l10n4x_tms::{export_file_bundle, import_file_bundle, SyncContext, SyncDirection};
use serde::Serialize;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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
        "file" => run_file_provider(config, direction, out, from)?,
        "webhook" => run_webhook_provider(config, direction)?,
        other if plugins::CORE_PROVIDERS.contains(&other) => {
            anyhow::bail!("Unhandled core provider '{other}'");
        }
        plugin_id => plugins::run_plugin_sync(plugin_id, config, direction, out, from)?,
    }
    Ok(())
}

fn run_file_provider(
    config: &Config,
    direction: SyncDirection,
    out: Option<&str>,
    from: Option<&str>,
) -> Result<(), anyhow::Error> {
    let ctx = sync_context(config, "file");
    match direction {
        SyncDirection::Export => {
            let dir = out.unwrap_or("tms-export");
            export_file_bundle(&ctx, Path::new(dir))?;
            println!("TMS export written to '{dir}'");
        }
        SyncDirection::Import => {
            let dir = from.ok_or_else(|| anyhow::anyhow!("--from is required for sync import"))?;
            import_file_bundle(&ctx, Path::new(dir))?;
            println!("TMS import merged into '{}'", config.source_dir);
        }
        SyncDirection::Push => {
            anyhow::bail!("file provider does not support push; use --provider webhook");
        }
    }
    Ok(())
}

fn run_webhook_provider(config: &Config, direction: SyncDirection) -> Result<(), anyhow::Error> {
    if direction != SyncDirection::Push {
        anyhow::bail!("webhook provider only supports --direction push");
    }
    push_webhook(config)?;
    println!("Signed paks pushed to webhook");
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

fn sync_context(config: &Config, _provider: &str) -> SyncContext {
    SyncContext {
        project: config.project.clone(),
        source_dir: config.source_dir.clone(),
        output_dir: config.output_dir.clone(),
        fallback: config.fallback.clone(),
        bundles_mode: config.bundles.mode.clone(),
        plugin_settings: serde_json::Value::Object(serde_json::Map::new()),
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
        req = req.set("Authorization", &format!("Bearer {token}"));
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
        anyhow::bail!(
            "no .pak artifacts found in '{}' — run `l10n4x build` first",
            config.output_dir
        );
    }
    artifacts.sort_by(|a, b| {
        (a.locale.as_str(), a.namespace.as_deref())
            .cmp(&(b.locale.as_str(), b.namespace.as_deref()))
    });
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
    digest.iter().map(|b| format!("{b:02x}")).collect()
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
    use l10n4x_tms::SyncDirection;

    #[test]
    fn sync_direction_parse() {
        assert_eq!(
            SyncDirection::parse("export").unwrap(),
            SyncDirection::Export
        );
        assert!(SyncDirection::parse("nope").is_err());
    }
}
