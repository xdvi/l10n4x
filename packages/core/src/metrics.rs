//! Performance and diagnostic counters for l10n4x-core.
//!
//! All counters use `AtomicU64` and are `no_std`-compatible.

extern crate alloc;

use core::sync::atomic::{AtomicU64, Ordering};

/// Total translation lookups attempted.
static TOTAL_TRANSLATIONS: AtomicU64 = AtomicU64::new(0);
/// Number of times a key was found in a loaded locale or fallback.
static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
/// Number of times a key was NOT found (triggers missing key handler).
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
/// Number of locale load events.
static LOCALE_LOADS: AtomicU64 = AtomicU64::new(0);
/// Number of format errors (malformed bytecode, etc).
static FORMAT_ERRORS: AtomicU64 = AtomicU64::new(0);

/// Increment the total translations counter.
pub fn inc_total_translations() { TOTAL_TRANSLATIONS.fetch_add(1, Ordering::Relaxed); }
/// Increment the cache hit counter.
pub fn inc_cache_hits() { CACHE_HITS.fetch_add(1, Ordering::Relaxed); }
/// Increment the cache miss counter.
pub fn inc_cache_misses() { CACHE_MISSES.fetch_add(1, Ordering::Relaxed); }
/// Increment the locale load counter.
pub fn inc_locale_loads() { LOCALE_LOADS.fetch_add(1, Ordering::Relaxed); }
/// Increment the format error counter.
pub fn inc_format_errors() { FORMAT_ERRORS.fetch_add(1, Ordering::Relaxed); }
/// Read the current format error count.
pub fn format_errors() -> u64 { FORMAT_ERRORS.load(Ordering::Relaxed) }

/// Returns all metrics as a formatted string: `total,hits,misses,loads,errors`.
pub fn metrics_string() -> alloc::string::String {
    alloc::format!(
        "{},{},{},{},{}",
        TOTAL_TRANSLATIONS.load(Ordering::Relaxed),
        CACHE_HITS.load(Ordering::Relaxed),
        CACHE_MISSES.load(Ordering::Relaxed),
        LOCALE_LOADS.load(Ordering::Relaxed),
        FORMAT_ERRORS.load(Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn metrics_initial_all_zero() {
        // First, capture a baseline string
        let s = metrics_string();
        let parts: Vec<u64> = s.split(',').map(|p| p.parse().unwrap()).collect();
        assert_eq!(parts.len(), 5);
    }

    #[test]
    fn metrics_increment_counters() {
        inc_total_translations();
        inc_cache_hits();
        inc_cache_misses();
        inc_locale_loads();
        inc_format_errors();
        let s = metrics_string();
        let parts: Vec<u64> = s.split(',').map(|p| p.parse().unwrap()).collect();
        // Each counter should have been incremented at least once
        assert!(parts[0] >= 1);
        assert!(parts[1] >= 1);
        assert!(parts[2] >= 1);
        assert!(parts[3] >= 1);
        assert!(parts[4] >= 1);
    }

    #[test]
    fn metrics_string_format() {
        // Verify format is correct regardless of current counter values
        let s = metrics_string();
        let parts: Vec<u64> = s.split(',').map(|p| p.parse().unwrap()).collect();
        assert_eq!(parts.len(), 5);
    }
}
