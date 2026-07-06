//! Crowdin-compatible locale tree export/import for l10n4x.

use ahash::AHashMap;
use l10n4x_compiler::flatten_value;
use l10n4x_tms::{
    scan_source_bundle, write_bundle_to_source, SyncContext, SyncDirection, TmsBundle, TmsProvider,
    TMS_FORMAT, TMS_VERSION,
};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub const PLUGIN_ID: &str = "crowdin";

/// Crowdin TMS plugin — offline tree exchange today; API sync in a future release.
#[derive(Debug, Default, Clone, Copy)]
pub struct CrowdinProvider;

impl TmsProvider for CrowdinProvider {
    fn id(&self) -> &'static str {
        PLUGIN_ID
    }

    fn export(&self, ctx: &SyncContext, out: &Path) -> Result<(), anyhow::Error> {
        export_crowdin_tree(ctx, out)
    }

    fn import(&self, ctx: &SyncContext, from: Option<&Path>) -> Result<(), anyhow::Error> {
        match from {
            Some(dir) => import_crowdin_tree(ctx, dir),
            None => try_api_import(ctx),
        }
    }

    fn push(&self, ctx: &SyncContext) -> Result<(), anyhow::Error> {
        try_api_push(ctx)
    }
}

pub fn run_sync(
    ctx: &SyncContext,
    direction: SyncDirection,
    out: Option<&Path>,
    from: Option<&Path>,
) -> Result<(), anyhow::Error> {
    let provider = CrowdinProvider;
    match direction {
        SyncDirection::Export => {
            let dir = out.unwrap_or(Path::new("tms-crowdin"));
            provider.export(ctx, dir)?;
            println!(
                "Crowdin-compatible tree written to '{}' (upload via Crowdin UI or API)",
                dir.display()
            );
        }
        SyncDirection::Import => provider.import(ctx, from)?,
        SyncDirection::Push => provider.push(ctx)?,
    }
    Ok(())
}

fn export_crowdin_tree(ctx: &SyncContext, out_dir: &Path) -> Result<(), anyhow::Error> {
    let bundle = scan_source_bundle(ctx)?;
    for (namespace, locales) in &bundle.namespaces {
        for (locale, flat) in locales {
            let nested = l10n4x_tms::unflatten_keys(flat);
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
        "Crowdin-compatible export from l10n4x (plugin: crowdin).\n\
         Upload each locale/*.json file to your Crowdin project.\n\
         After translation, download and run:\n\
           l10n4x sync --provider crowdin --direction import --from <download-dir>\n\
         Or configure plugins.crowdin.projectId + tokenEnv for future API pull.\n",
    )?;
    Ok(())
}

fn import_crowdin_tree(ctx: &SyncContext, from_dir: &Path) -> Result<(), anyhow::Error> {
    if !from_dir.is_dir() {
        anyhow::bail!("'{}' is not a directory", from_dir.display());
    }
    let mut namespaces: HashMap<String, HashMap<String, HashMap<String, String>>> = HashMap::new();

    for locale_entry in fs::read_dir(from_dir)? {
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
            let mut flat = AHashMap::new();
            flatten_value(namespace.clone(), &parsed, &mut flat);
            namespaces
                .entry(namespace)
                .or_default()
                .insert(locale.clone(), flat.into_iter().collect());
        }
    }

    let bundle = TmsBundle {
        format: TMS_FORMAT.to_string(),
        version: TMS_VERSION,
        project: ctx.project.clone(),
        fallback: ctx.fallback.clone(),
        exported_at: format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        ),
        namespaces,
    };
    write_bundle_to_source(ctx, &bundle)?;
    println!("Crowdin tree imported into '{}'", ctx.source_dir);
    Ok(())
}

fn crowdin_token(ctx: &SyncContext) -> Option<String> {
    let env_key = ctx
        .plugin_settings
        .get("tokenEnv")
        .or_else(|| ctx.plugin_settings.get("crowdinTokenEnv"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())?;
    std::env::var(env_key).ok()
}

fn crowdin_project_id(ctx: &SyncContext) -> Option<String> {
    ctx.plugin_settings
        .get("projectId")
        .or_else(|| ctx.plugin_settings.get("crowdinProjectId"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn try_api_import(ctx: &SyncContext) -> Result<(), anyhow::Error> {
    if crowdin_project_id(ctx).is_some() && crowdin_token(ctx).is_some() {
        anyhow::bail!(
            "Crowdin API import is not implemented yet. Download translations from Crowdin and run:\n\
             l10n4x sync --provider crowdin --direction import --from <download-dir>"
        );
    }
    anyhow::bail!(
        "--from is required for crowdin import, or configure plugins.crowdin.projectId and tokenEnv for API pull (coming soon)"
    );
}

fn try_api_push(ctx: &SyncContext) -> Result<(), anyhow::Error> {
    if crowdin_project_id(ctx).is_some() && crowdin_token(ctx).is_some() {
        anyhow::bail!(
            "Crowdin API push is not implemented yet. Export and upload manually:\n\
             l10n4x sync --provider crowdin --direction export --out ./tms-crowdin"
        );
    }
    anyhow::bail!(
        "crowdin push requires Crowdin API credentials in plugins.crowdin (projectId, tokenEnv).\n\
         For now use: l10n4x sync --provider crowdin --direction export\n\
         Or push signed lpks with: l10n4x sync --provider webhook --direction push"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use l10n4x_tms::export_file_bundle;
    use std::fs;
    use tempfile::tempdir;

    fn sample_ctx(dir: &Path) -> SyncContext {
        SyncContext {
            project: "demo".to_string(),
            source_dir: dir.join("locales").display().to_string(),
            output_dir: dir.join("dist").display().to_string(),
            fallback: "en".to_string(),
            bundles_mode: "monolith".to_string(),
            plugin_settings: serde_json::json!({}),
        }
    }

    #[test]
    fn crowdin_export_import_roundtrip() {
        let root = tempdir().unwrap();
        let locales = root.path().join("locales/en");
        fs::create_dir_all(&locales).unwrap();
        fs::write(locales.join("common.json"), r#"{"welcome":{"title":"Hi"}}"#).unwrap();

        let ctx = sample_ctx(root.path());
        let export_dir = root.path().join("crowdin-out");
        export_crowdin_tree(&ctx, &export_dir).unwrap();
        assert!(export_dir.join("en/common.json").exists());

        fs::remove_dir_all(&locales).unwrap();
        import_crowdin_tree(&ctx, &export_dir).unwrap();
        let restored = fs::read_to_string(locales.join("common.json")).unwrap();
        assert!(restored.contains("Hi"));
    }

    #[test]
    fn file_bundle_still_available_via_tms_crate() {
        let root = tempdir().unwrap();
        let locales = root.path().join("locales/en");
        fs::create_dir_all(&locales).unwrap();
        fs::write(locales.join("common.json"), r#"{"x":"y"}"#).unwrap();
        let ctx = sample_ctx(root.path());
        let out = root.path().join("handoff");
        export_file_bundle(&ctx, &out).unwrap();
        assert!(out.join("l10n4x-tms.json").exists());
    }
}
