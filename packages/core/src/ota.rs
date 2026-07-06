//! OTA (over-the-air) translation pak reload with one-retired-snapshot rollback.

extern crate alloc;

use crate::error::CoreResult;
use crate::pak::decompress_pak;
use crate::store::{
    build_store, store_snapshot, upsert_locale, LazyDecompressCache, OffsetMap, StoreData,
    StoreSnapshot,
};
#[cfg(feature = "std")]
use crate::store_registry::StoreHandle;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;

#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::sync::{Mutex, OnceLock};

#[cfg(feature = "std")]
type LazyCacheEntry = Arc<OnceLock<(Vec<u8>, OffsetMap)>>;

/// Per-locale retired snapshot used for OTA rollback (one generation per locale).
#[cfg(feature = "std")]
pub struct LocaleRetiredSnapshot {
    /// Locale bundle bytes (owned, static, or lazy).
    pub store_data: StoreData,
    /// Per-locale offset map entry, if any.
    pub offset_map: Option<OffsetMap>,
    /// Per-locale lazy decompression cache entry, if any.
    pub lazy_cache_entry: Option<LazyCacheEntry>,
    /// Loaded namespace list for modular bundles, if any.
    pub loaded_namespaces: Option<Arc<[Arc<str>]>>,
}

#[cfg(feature = "std")]
type RetiredKey = (u32, String);

#[cfg(feature = "std")]
fn store_id_from_handle(handle: Option<StoreHandle>) -> u32 {
    handle.map(|h| h.raw()).unwrap_or(0)
}

#[cfg(feature = "std")]
fn retired_snapshots() -> &'static Mutex<HashMap<RetiredKey, LocaleRetiredSnapshot>> {
    static MAP: OnceLock<Mutex<HashMap<RetiredKey, LocaleRetiredSnapshot>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(feature = "std")]
fn capture_locale_snapshot(snap: &StoreSnapshot, locale: &str) -> Option<LocaleRetiredSnapshot> {
    let locale_hash = crate::binary_format::fnv1a_64(locale.as_bytes());
    let idx = snap
        .locales
        .binary_search_by(|(loc, _)| loc.as_str().cmp(locale))
        .ok()?;
    let store_data = snap.locales[idx].1.as_ref().clone();
    let offset_map = snap
        .offset_maps
        .as_ref()
        .and_then(|m| m.get(&locale_hash).cloned());
    let lazy_cache_entry = snap
        .lazy_cache
        .as_ref()
        .and_then(|c| c.get(&locale_hash).cloned());
    let loaded_namespaces = snap
        .loaded_namespaces
        .as_ref()
        .and_then(|m| m.get(&locale_hash).cloned());
    Some(LocaleRetiredSnapshot {
        store_data,
        offset_map,
        lazy_cache_entry,
        loaded_namespaces,
    })
}

#[cfg(feature = "std")]
fn save_retired_snapshot(store_id: u32, locale: &str, retired: LocaleRetiredSnapshot) {
    if let Ok(mut map) = retired_snapshots().lock() {
        map.insert((store_id, locale.to_string()), retired);
    }
}

#[cfg(feature = "std")]
fn clone_map_remove_key<K: Eq + std::hash::Hash + Clone, V: Clone>(
    map: &Option<Arc<HashMap<K, V>>>,
    key: &K,
) -> Option<Arc<HashMap<K, V>>> {
    let arc = map.as_ref()?;
    if !arc.contains_key(key) {
        return map.clone();
    }
    let mut new_map = (**arc).clone();
    new_map.remove(key);
    if new_map.is_empty() {
        None
    } else {
        Some(Arc::new(new_map))
    }
}

#[cfg(feature = "std")]
fn clone_map_upsert<K: Eq + std::hash::Hash + Clone, V: Clone>(
    map: &mut Option<Arc<HashMap<K, V>>>,
    key: K,
    value: V,
) {
    let mut new_map = map.as_ref().map(|m| (**m).clone()).unwrap_or_default();
    new_map.insert(key, value);
    *map = Some(Arc::new(new_map));
}

#[cfg(feature = "std")]
fn restore_locale_snapshot(snap: &mut StoreSnapshot, locale: &str, retired: LocaleRetiredSnapshot) {
    let locale_hash = crate::binary_format::fnv1a_64(locale.as_bytes());
    upsert_locale(&mut snap.locales, locale.to_string(), retired.store_data);

    match retired.offset_map {
        Some(om) => clone_map_upsert(&mut snap.offset_maps, locale_hash, om),
        None => snap.offset_maps = clone_map_remove_key(&snap.offset_maps, &locale_hash),
    }

    match retired.lazy_cache_entry {
        Some(entry) => {
            let lazy = snap
                .lazy_cache
                .get_or_insert_with(|| Arc::new(LazyDecompressCache::new()));
            let mut new_lazy = (**lazy).clone();
            new_lazy.insert(locale_hash, entry);
            snap.lazy_cache = Some(Arc::new(new_lazy));
        }
        None => snap.lazy_cache = clone_map_remove_key(&snap.lazy_cache, &locale_hash),
    }

    match retired.loaded_namespaces {
        Some(ns) => clone_map_upsert(&mut snap.loaded_namespaces, locale_hash, ns),
        None => {
            snap.loaded_namespaces = clone_map_remove_key(&snap.loaded_namespaces, &locale_hash)
        }
    }
}

/// Returns `true` when a retired snapshot exists for `handle` + `locale` and rollback is possible.
#[cfg(feature = "std")]
pub fn ota_can_rollback_for_store(handle: Option<StoreHandle>, locale: &str) -> bool {
    let store_id = store_id_from_handle(handle);
    retired_snapshots()
        .lock()
        .map(|m| m.contains_key(&(store_id, locale.to_string())))
        .unwrap_or(false)
}

/// Returns `true` when a retired snapshot exists for `locale` and rollback is possible.
#[cfg(feature = "std")]
pub fn ota_can_rollback(locale: &str) -> bool {
    ota_can_rollback_for_store(None, locale)
}

#[cfg(not(feature = "std"))]
pub fn ota_can_rollback(_locale: &str) -> bool {
    false
}

/// Verifies `pak_bytes`, saves the current locale state for rollback, and atomically loads the new pak.
#[cfg(feature = "std")]
pub fn try_ota_reload_pak_for_store(
    handle: Option<StoreHandle>,
    locale: &str,
    pak_bytes: &[u8],
) -> CoreResult<()> {
    let decompressed =
        decompress_pak(pak_bytes).inspect_err(|_| crate::metrics::inc_pak_verify_failures())?;

    // Capture-for-rollback and load must happen in ONE writer critical
    // section: a concurrent load landing between them would make the rollback
    // snapshot restore the wrong state.
    crate::store::update_store_for(handle, |store| {
        let mut snap = store_snapshot(store);
        if let Some(retired) = capture_locale_snapshot(&snap, locale) {
            save_retired_snapshot(store_id_from_handle(handle), locale, retired);
        }
        crate::loader::apply_l10n_to_snapshot(
            &mut snap,
            locale,
            decompressed,
            crate::loader::LoadMode::Replace,
        )?;
        Ok(build_store(snap))
    })?;
    crate::store::notify_locale_changed_for_handle(handle, locale);
    crate::metrics::inc_pak_reload_total();
    Ok(())
}

/// Verifies `pak_bytes`, saves the current locale state for rollback, and atomically loads the new pak.
#[cfg(feature = "std")]
pub fn try_ota_reload_pak(locale: &str, pak_bytes: &[u8]) -> CoreResult<()> {
    try_ota_reload_pak_for_store(None, locale, pak_bytes)
}

#[cfg(not(feature = "std"))]
pub fn try_ota_reload_pak(_locale: &str, _pak_bytes: &[u8]) -> CoreResult<()> {
    Err(crate::CoreError::IoError("OTA requires std feature"))
}

/// Restores the retired snapshot for `handle` + `locale` when present.
#[cfg(feature = "std")]
pub fn try_ota_rollback_for_store(handle: Option<StoreHandle>, locale: &str) -> CoreResult<()> {
    let store_id = store_id_from_handle(handle);
    let retired = retired_snapshots()
        .lock()
        .ok()
        .and_then(|mut m| m.remove(&(store_id, locale.to_string())));
    let Some(retired) = retired else {
        return Err(crate::CoreError::InvalidFormat("no OTA rollback snapshot"));
    };

    crate::store::update_store_for(handle, |store| {
        let mut snap = store_snapshot(store);
        restore_locale_snapshot(&mut snap, locale, retired);
        Ok(build_store(snap))
    })?;
    crate::metrics::inc_pak_rollback_total();
    crate::store::notify_locale_changed_for_handle(handle, locale);
    Ok(())
}

/// Restores the retired snapshot for `locale` when present.
#[cfg(feature = "std")]
pub fn try_ota_rollback(locale: &str) -> CoreResult<()> {
    try_ota_rollback_for_store(None, locale)
}

#[cfg(not(feature = "std"))]
pub fn try_ota_rollback(_locale: &str) -> CoreResult<()> {
    Err(crate::CoreError::IoError("OTA requires std feature"))
}

/// Convenience wrapper returning `true` on successful OTA reload.
#[cfg(feature = "std")]
pub fn ota_reload_pak(locale: &str, pak_bytes: &[u8]) -> bool {
    try_ota_reload_pak(locale, pak_bytes).is_ok()
}

#[cfg(not(feature = "std"))]
pub fn ota_reload_pak(_locale: &str, _pak_bytes: &[u8]) -> bool {
    false
}

/// Convenience wrapper returning `true` on successful rollback.
#[cfg(feature = "std")]
pub fn ota_rollback(locale: &str) -> bool {
    try_ota_rollback(locale).is_ok()
}

#[cfg(not(feature = "std"))]
pub fn ota_rollback(_locale: &str) -> bool {
    false
}
