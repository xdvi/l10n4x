//! Optional TMS plugin registry (`crowdin`, future: `lokalise`, …).

mod validate;

use crate::config::Config;
use l10n4x_tms::{SyncContext, SyncDirection};
use std::path::Path;
use std::process::{Command, Stdio};

pub use validate::{discover_plugin_ids_on_path, is_valid_plugin_id, validate_plugins};

pub const CORE_PROVIDERS: &[&str] = &["file", "webhook"];

/// Known optional plugins and how to install them.
pub const KNOWN_PLUGINS: &[(&str, &str)] = &[(
    "crowdin",
    "cargo install l10n4x-plugin-crowdin",
)];

pub fn list_plugins() {
    println!("Core TMS providers (built into l10n4x):");
    for id in CORE_PROVIDERS {
        println!("  - {id}");
    }

    let discovered = discover_plugin_ids_on_path();
    if !discovered.is_empty() {
        println!("\nDiscovered on PATH (l10n4x-plugin-<id>):");
        for id in &discovered {
            println!("  - {id}");
        }
    }

    println!("\nDocumented optional plugins:");
    for (id, install) in KNOWN_PLUGINS {
        let linked = in_process_plugin(id);
        let status = if linked {
            "linked"
        } else if plugin_binary_on_path(id) {
            "installed (PATH)"
        } else {
            "not installed"
        };
        println!("  - {id} [{status}]");
        println!("    install: {install}");
    }
    println!("\nValidate: l10n4x plugin validate [id]");
}

pub fn plugin_info(name: &str) -> Result<(), anyhow::Error> {
    if !is_valid_plugin_id(name) {
        anyhow::bail!("invalid plugin id '{name}'");
    }
    if CORE_PROVIDERS.contains(&name) {
        anyhow::bail!("'{name}' is a core TMS provider, not a plugin");
    }

    let install = KNOWN_PLUGINS
        .iter()
        .find(|(id, _)| *id == name)
        .map(|(_, cmd)| *cmd)
        .unwrap_or("place an executable l10n4x-plugin-<id> on PATH");

    println!("Plugin: {name}");
    println!("Binary: l10n4x-plugin-{name}");
    println!("Install: {install}");
    println!("Validate: l10n4x plugin validate {name}");
    println!("Usage: l10n4x sync --provider {name} --direction export|import|push");
    println!("Config: plugins.{name} in l10n4x.config.json");
    println!("\nContract:");
    println!("  l10n4x-plugin-{name} sync <export|import|push> --config l10n4x.config.json [--out DIR] [--from DIR]");
    println!("  l10n4x-plugin-{name} info   (recommended)");
    println!("  l10n4x-plugin-{name} --help  (must exit 0)");
    Ok(())
}

pub fn run_plugin_sync(
    plugin_id: &str,
    config: &Config,
    direction: SyncDirection,
    out: Option<&str>,
    from: Option<&str>,
) -> Result<(), anyhow::Error> {
    if !is_valid_plugin_id(plugin_id) {
        anyhow::bail!("invalid plugin id '{plugin_id}'");
    }
    if CORE_PROVIDERS.contains(&plugin_id) {
        anyhow::bail!("'{plugin_id}' is a core provider; use l10n4x sync --provider {plugin_id}");
    }

    let ctx = sync_context_from_config(config, plugin_id);

    if in_process_plugin(plugin_id) {
        return run_in_process_plugin(plugin_id, &ctx, direction, out, from);
    }

    if plugin_binary_on_path(plugin_id) {
        return run_plugin_subprocess(plugin_id, direction, out, from);
    }

    let install_hint = KNOWN_PLUGINS
        .iter()
        .find(|(id, _)| *id == plugin_id)
        .map(|(_, cmd)| *cmd)
        .unwrap_or("install l10n4x-plugin-<id> on PATH (see: l10n4x plugin validate)");

    anyhow::bail!(
        "TMS plugin '{plugin_id}' is not installed.\n\
         Install it, then retry:\n  {install_hint}\n\
         Validate: l10n4x plugin validate {plugin_id}\n\
         Or use a core provider: {}",
        CORE_PROVIDERS.join(", ")
    );
}

fn sync_context_from_config(config: &Config, plugin_id: &str) -> SyncContext {
    SyncContext {
        project: config.project.clone(),
        source_dir: config.source_dir.clone(),
        output_dir: config.output_dir.clone(),
        fallback: config.fallback.clone(),
        bundles_mode: config.bundles.mode.clone(),
        plugin_settings: config
            .plugins
            .get(plugin_id)
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
    }
}

fn run_in_process_plugin(
    plugin_id: &str,
    ctx: &SyncContext,
    direction: SyncDirection,
    out: Option<&str>,
    from: Option<&str>,
) -> Result<(), anyhow::Error> {
    match plugin_id {
        #[cfg(feature = "plugin-crowdin")]
        "crowdin" => l10n4x_plugin_crowdin::run_sync(
            ctx,
            direction,
            out.map(Path::new),
            from.map(Path::new),
        ),
        _ => anyhow::bail!("plugin '{plugin_id}' is not linked into this l10n4x build"),
    }
}

pub(crate) fn in_process_plugin(id: &str) -> bool {
    match id {
        #[cfg(feature = "plugin-crowdin")]
        "crowdin" => true,
        _ => false,
    }
}

pub(crate) fn plugin_binary_name(id: &str) -> String {
    format!("l10n4x-plugin-{id}")
}

fn plugin_binary_on_path(id: &str) -> bool {
    which_plugin_binary(id).is_some()
}

fn which_plugin_binary(id: &str) -> Option<std::ffi::OsString> {
    let name = plugin_binary_name(id);
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(&name);
            if candidate.is_file() {
                Some(candidate.into_os_string())
            } else {
                None
            }
        })
    })
}

fn run_plugin_subprocess(
    plugin_id: &str,
    direction: SyncDirection,
    out: Option<&str>,
    from: Option<&str>,
) -> Result<(), anyhow::Error> {
    let bin = which_plugin_binary(plugin_id).ok_or_else(|| {
        anyhow::anyhow!("plugin binary l10n4x-plugin-{plugin_id} not found on PATH")
    })?;

    let mut cmd = Command::new(bin);
    cmd.arg("sync")
        .arg(direction.as_str())
        .arg("--config")
        .arg("l10n4x.config.json");

    if let Some(out_dir) = out {
        cmd.arg("--out").arg(out_dir);
    }
    if let Some(from_dir) = from {
        cmd.arg("--from").arg(from_dir);
    }

    cmd.stdin(Stdio::null());
    let status = cmd.status()?;
    if !status.success() {
        anyhow::bail!("plugin '{plugin_id}' exited with status {status}");
    }
    Ok(())
}