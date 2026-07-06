//! Performance and diagnostic counters for l10n4x-core.
//!
//! All counters use `AtomicU64` and are `no_std`-compatible.
//!
//! # Extended metrics string format (v2)
//!
//! ```text
//! v2,{total},{hits},{misses},{loads},{errors},{pak_reload},{pak_verify_fail},{pak_rollback},{hit_ratio},{miss_by_locale}
//! ```
//!
//! - `hit_ratio` — cache hits / total translations (0.0 when total is 0).
//! - `miss_by_locale` — pipe-separated `locale:count` pairs (e.g. `en:3|es:1`), empty when none.

extern crate alloc;

use core::sync::atomic::{AtomicU64, Ordering};

#[cfg(feature = "std")]
use alloc::string::String;
#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::sync::Mutex;

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
/// Successful OTA pak reloads.
static PAK_RELOAD_TOTAL: AtomicU64 = AtomicU64::new(0);
/// OTA pak signature / verification failures.
static PAK_VERIFY_FAILURES: AtomicU64 = AtomicU64::new(0);
/// OTA rollbacks to a retired snapshot.
static PAK_ROLLBACK_TOTAL: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "std")]
static MISS_BY_LOCALE: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

#[cfg(feature = "std")]
fn miss_map_mut() -> std::sync::MutexGuard<'static, Option<HashMap<String, u64>>> {
    MISS_BY_LOCALE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Increment the total translations counter.
pub fn inc_total_translations() {
    TOTAL_TRANSLATIONS.fetch_add(1, Ordering::Relaxed);
}
/// Increment the cache hit counter.
pub fn inc_cache_hits() {
    CACHE_HITS.fetch_add(1, Ordering::Relaxed);
}
/// Increment the cache miss counter.
pub fn inc_cache_misses() {
    CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
}
/// Upper bound on distinct locales tracked per-locale; misses beyond it still
/// count in the global counter. Prevents unbounded growth when locale strings
/// are caller-controlled.
#[cfg(feature = "std")]
const MISS_BY_LOCALE_CAP: usize = 256;

/// Increment the cache miss counter and per-locale miss tracking.
#[cfg(feature = "std")]
pub fn inc_cache_misses_for_locale(locale: &str) {
    CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
    let mut guard = miss_map_mut();
    let map = guard.get_or_insert_with(HashMap::new);
    // get_mut first: allocate the key String only on the first miss per locale.
    if let Some(count) = map.get_mut(locale) {
        *count += 1;
    } else if map.len() < MISS_BY_LOCALE_CAP {
        map.insert(locale.to_string(), 1);
    }
}

/// Increment cache misses for a locale (no_std stub).
#[cfg(not(feature = "std"))]
pub fn inc_cache_misses_for_locale(_locale: &str) {
    inc_cache_misses();
}

/// Increment the locale load counter.
pub fn inc_locale_loads() {
    LOCALE_LOADS.fetch_add(1, Ordering::Relaxed);
}
/// Increment the format error counter.
pub fn inc_format_errors() {
    FORMAT_ERRORS.fetch_add(1, Ordering::Relaxed);
}
/// Increment successful OTA pak reload counter.
pub fn inc_pak_reload_total() {
    PAK_RELOAD_TOTAL.fetch_add(1, Ordering::Relaxed);
}
/// Increment OTA pak verification failure counter.
pub fn inc_pak_verify_failures() {
    PAK_VERIFY_FAILURES.fetch_add(1, Ordering::Relaxed);
}
/// Increment OTA rollback counter.
pub fn inc_pak_rollback_total() {
    PAK_ROLLBACK_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// Read the current format error count.
pub fn format_errors() -> u64 {
    FORMAT_ERRORS.load(Ordering::Relaxed)
}

/// Returns cache hit ratio in `[0.0, 1.0]` (0.0 when no translations yet).
#[cfg(feature = "std")]
pub fn cache_hit_ratio() -> f64 {
    let total = TOTAL_TRANSLATIONS.load(Ordering::Relaxed);
    if total == 0 {
        return 0.0;
    }
    let hits = CACHE_HITS.load(Ordering::Relaxed);
    hits as f64 / total as f64
}

/// Returns all metrics as a formatted string (legacy 5-field format).
pub fn metrics_string_legacy() -> alloc::string::String {
    alloc::format!(
        "{},{},{},{},{}",
        TOTAL_TRANSLATIONS.load(Ordering::Relaxed),
        CACHE_HITS.load(Ordering::Relaxed),
        CACHE_MISSES.load(Ordering::Relaxed),
        LOCALE_LOADS.load(Ordering::Relaxed),
        FORMAT_ERRORS.load(Ordering::Relaxed)
    )
}

/// Returns extended v2 metrics (see module docs).
#[cfg(feature = "std")]
pub fn metrics_string() -> String {
    let miss_by_locale = {
        let guard = miss_map_mut();
        guard
            .as_ref()
            .map(|m| {
                let mut pairs: Vec<_> = m.iter().collect();
                pairs.sort_by(|a, b| a.0.cmp(b.0));
                pairs
                    .into_iter()
                    .map(|(loc, count)| alloc::format!("{loc}:{count}"))
                    .collect::<Vec<_>>()
                    .join("|")
            })
            .unwrap_or_default()
    };
    alloc::format!(
        "v2,{},{},{},{},{},{},{},{},{:.6},{}",
        TOTAL_TRANSLATIONS.load(Ordering::Relaxed),
        CACHE_HITS.load(Ordering::Relaxed),
        CACHE_MISSES.load(Ordering::Relaxed),
        LOCALE_LOADS.load(Ordering::Relaxed),
        FORMAT_ERRORS.load(Ordering::Relaxed),
        PAK_RELOAD_TOTAL.load(Ordering::Relaxed),
        PAK_VERIFY_FAILURES.load(Ordering::Relaxed),
        PAK_ROLLBACK_TOTAL.load(Ordering::Relaxed),
        cache_hit_ratio(),
        miss_by_locale
    )
}

/// Return a human-readable metrics summary (no_std stub).
#[cfg(not(feature = "std"))]
pub fn metrics_string() -> alloc::string::String {
    metrics_string_legacy()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn metrics_initial_all_zero() {
        let s = metrics_string_legacy();
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
        let s = metrics_string_legacy();
        let parts: Vec<u64> = s.split(',').map(|p| p.parse().unwrap()).collect();
        assert!(parts[0] >= 1);
        assert!(parts[1] >= 1);
        assert!(parts[2] >= 1);
        assert!(parts[3] >= 1);
        assert!(parts[4] >= 1);
    }

    #[test]
    fn metrics_string_format() {
        let s = metrics_string_legacy();
        let parts: Vec<u64> = s.split(',').map(|p| p.parse().unwrap()).collect();
        assert_eq!(parts.len(), 5);
    }

    #[cfg(feature = "std")]
    #[test]
    fn extended_metrics_v2_prefix() {
        let s = metrics_string();
        assert!(s.starts_with("v2,"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn miss_by_locale_tracked() {
        inc_cache_misses_for_locale("en");
        inc_cache_misses_for_locale("en");
        inc_cache_misses_for_locale("es");
        let s = metrics_string();
        assert!(s.contains("en:2"));
        assert!(s.contains("es:1"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn cache_hit_ratio_bounded() {
        let ratio = cache_hit_ratio();
        assert!((0.0..=1.0).contains(&ratio));
    }

    #[test]
    fn pak_metrics_increment() {
        inc_pak_reload_total();
        inc_pak_verify_failures();
        inc_pak_rollback_total();
        #[cfg(feature = "std")]
        {
            let s = metrics_string();
            let parts: Vec<&str> = s.split(',').collect();
            assert!(parts.len() >= 9);
            let reload: u64 = parts[6].parse().unwrap();
            let verify_fail: u64 = parts[7].parse().unwrap();
            let rollback: u64 = parts[8].parse().unwrap();
            assert!(reload >= 1);
            assert!(verify_fail >= 1);
            assert!(rollback >= 1);
        }
    }
}
