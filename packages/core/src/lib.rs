//! # l10n4x-core
//!
//! `l10n4x-core` is the high-performance, `#![no_std]` compatible runtime library
//! that processes and formats localization strings directly from decompressed in-memory `.pak` files.
//!
//! Key capabilities:
//! - Zero-copy decoding of compact, sorted binary localization packages.
//! - Fast O(log N) binary search lookups.
//! - Zero-allocation message formatting supporting ICU MessageFormat (plurals, select, variables).
//! - Lock-free quiescent RCU pointer swapping for runtime hot-reloads.

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

/// Structured error types for core operations.
pub mod error;
pub use error::{CoreError, CoreResult};

/// Custom binary package format parsing and search routines.
pub mod binary_format;
/// Locale-aware date and time formatting (date, time, datetime styles).
pub mod date_format;
/// Optional AES-GCM encryption (`L10E` envelope).
#[cfg(feature = "encryption")]
pub mod encryption;
/// Optional encrypted outer wrapper around signed paks.
pub mod envelope;
/// ICU MessageFormat parsing and interpolation engine.
pub mod formatter;
/// Ed25519 signing and verification for `.pak` integrity.
pub mod integrity;
/// Locale-aware list formatting ("A, B, and C").
pub mod list_format;
/// Decompression and in-memory pak loading.
pub mod loader;
/// Locale-aware number formatting (decimal, percent, integer styles).
pub mod number_format;
/// Outer `.pak` container format (zstd + Ed25519).
pub mod pak;
/// CLDR-accurate plural rule resolution for 120+ locales.
pub mod plural_rules;
pub(crate) mod locale_util;
pub(crate) mod reclaim;
/// Locale-aware relative time formatting (seconds ago, in X days, etc.).
pub mod reltime;
/// Thread-safe RCU store swap and lookup management.
pub mod store;
pub use store::{
    init_embedded, load_static_bytes, locale_parent, translate_to_writer_with_status,
    TranslateStatus, StoreData,
};
#[cfg(test)]
pub(crate) mod test_fixtures;

/// Diagnostic counters for telemetry.
pub mod metrics;

#[cfg(feature = "std")]
pub use formatter::{format_with_custom, register_formatter, CustomFormatter};

/// Macro helper to build a zero-cost stack-allocated slice of key-value parameters.
/// Useful for passing variables to the translation function without heap allocations.
///
/// # Example
/// ```
/// use l10n4x_core::l10n_params;
/// let params = l10n_params! { "name" => "Diego", "count" => "5" };
/// ```
#[macro_export]
macro_rules! l10n_params {
    ($($key:expr => $val:expr),* $(,)?) => {
        &[$(($key, $val)),*] as &[(&str, &str)]
    };
}
