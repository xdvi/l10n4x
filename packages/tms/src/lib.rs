//! TMS plugin API: exchange locale JSON with translation tools.
//!
//! Core CLI providers (`file`, `webhook`) live in `l10n4x-toolkit`.
//! Optional integrations (Crowdin, Lokalise, …) ship as separate plugin crates.

mod bundle;
mod provider;

pub use bundle::{
    export_file_bundle, import_file_bundle, scan_source_bundle, unflatten_keys,
    write_bundle_to_source, TmsBundle, TMS_FORMAT, TMS_VERSION,
};
pub use provider::{SyncContext, SyncDirection, TmsProvider};