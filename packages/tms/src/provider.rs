use serde_json::Value;
use std::path::Path;

/// Sync direction for TMS exchange.
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

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Export => "export",
            Self::Import => "import",
            Self::Push => "push",
        }
    }
}

/// Runtime context passed to TMS plugins (subset of `l10n4x.config.json`).
#[derive(Debug, Clone)]
pub struct SyncContext {
    pub project: String,
    pub source_dir: String,
    pub output_dir: String,
    pub fallback: String,
    pub bundles_mode: String,
    /// Provider-specific settings from `plugins.<id>` in config.
    pub plugin_settings: Value,
}

/// Optional TMS integration (Crowdin, Lokalise, …).
pub trait TmsProvider: Send + Sync {
    fn id(&self) -> &'static str;

    fn export(&self, ctx: &SyncContext, out: &Path) -> Result<(), anyhow::Error>;

    /// `from` is set for manual directory import; `None` means plugin may pull via API.
    fn import(&self, ctx: &SyncContext, from: Option<&Path>) -> Result<(), anyhow::Error>;

    fn push(&self, ctx: &SyncContext) -> Result<(), anyhow::Error>;
}