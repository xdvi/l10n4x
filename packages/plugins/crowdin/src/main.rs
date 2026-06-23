//! Standalone Crowdin TMS plugin binary (optional install alongside `l10n4x`).

use anyhow::Context;
use clap::{Parser, Subcommand};
use l10n4x_plugin_crowdin::{run_sync, PLUGIN_ID};
use l10n4x_tms::{SyncContext, SyncDirection};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "l10n4x-plugin-crowdin",
    about = "Crowdin TMS plugin for l10n4x",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a TMS sync operation (invoked by `l10n4x sync --provider crowdin` or directly).
    Sync {
        #[arg(value_name = "DIRECTION")]
        direction: String,
        #[arg(long, default_value = "l10n4x.config.json")]
        config: PathBuf,
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
        #[arg(long, value_name = "DIR")]
        from: Option<PathBuf>,
    },
    /// Show plugin metadata.
    Info,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginConfig {
    project: String,
    source_dir: String,
    output_dir: String,
    fallback: String,
    #[serde(default)]
    bundles: Option<BundlesSection>,
    #[serde(default)]
    plugins: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct BundlesSection {
    #[serde(default = "default_mode")]
    mode: String,
}

fn default_mode() -> String {
    "monolith".to_string()
}

fn load_sync_context(path: &Path) -> Result<SyncContext, anyhow::Error> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let cfg: PluginConfig = serde_json::from_str(&raw)?;
    let plugin_settings = cfg
        .plugins
        .get(PLUGIN_ID)
        .cloned()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    Ok(SyncContext {
        project: cfg.project,
        source_dir: cfg.source_dir,
        output_dir: cfg.output_dir,
        fallback: cfg.fallback,
        bundles_mode: cfg
            .bundles
            .map(|b| b.mode)
            .unwrap_or_else(default_mode),
        plugin_settings,
    })
}

fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Info => {
            println!("l10n4x TMS plugin: crowdin");
            println!("id: {PLUGIN_ID}");
            println!("install: cargo install l10n4x-plugin-crowdin");
            println!("usage: l10n4x sync --provider crowdin --direction export|import|push");
        }
        Commands::Sync {
            direction,
            config,
            out,
            from,
        } => {
            let ctx = load_sync_context(&config)?;
            let dir = SyncDirection::parse(&direction)?;
            run_sync(
                &ctx,
                dir,
                out.as_deref(),
                from.as_deref(),
            )?;
        }
    }
    Ok(())
}