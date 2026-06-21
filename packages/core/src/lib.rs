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

extern crate alloc;

/// Custom binary package format parsing and search routines.
pub mod binary_format;
/// Optional AES-GCM encryption (`L10E` envelope).
#[cfg(feature = "encryption")]
pub mod encryption;
/// Optional encrypted outer wrapper around signed paks.
pub mod envelope;
/// ICU MessageFormat parsing and interpolation engine.
pub mod formatter;
/// CLDR-accurate plural rule resolution for 120+ locales.
pub mod plural_rules;
/// Ed25519 signing and verification for `.pak` integrity.
pub mod integrity;
/// Decompression and in-memory pak loading.
pub mod loader;
/// Outer `.pak` container format (DEFLATE + Ed25519).
pub mod pak;
pub(crate) mod reclaim;
/// Thread-safe RCU store swap and lookup management.
pub mod store;
#[cfg(test)]
pub(crate) mod test_fixtures;

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
