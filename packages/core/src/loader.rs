extern crate alloc;
use crate::error::CoreResult;
use crate::pak::decompress_pak;
#[cfg(not(feature = "std"))]
use crate::store::TranslationStore;
#[cfg(feature = "std")]
use crate::store::{
    build_store, lazy_cache_mut, notify_locale_changed_for_handle, offset_maps_mut,
    store_snapshot, update_store_for, StoreSnapshot,
};
use crate::store::{emit_locale_changed, upsert_locale, StoreData};
#[cfg(feature = "std")]
use crate::store_registry::StoreHandle;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::vec::Vec;

#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::sync::OnceLock;

#[cfg(feature = "std")]
pub(crate) enum LoadMode<'a> {
    Replace,
    Namespace(&'a str),
}

#[cfg(feature = "std")]
fn merge_debug_keys(snap: &mut StoreSnapshot, bytes: &[u8], replace: bool) -> CoreResult<()> {
    #[cfg(feature = "debug-keys")]
    {
        let reader = crate::binary_format::BinaryFormatReader::new(bytes)?;
        let table = reader.debug_key_table();
        if table.is_empty() {
            return Ok(());
        }
        if replace {
            snap.debug_keys = None;
        }
        let dk = snap
            .debug_keys
            .get_or_insert_with(|| Arc::new(HashMap::new()));
        let map = Arc::make_mut(dk);
        for (hash, name) in table {
            map.insert(hash, Arc::from(name.as_str()));
        }
    }
    #[cfg(not(feature = "debug-keys"))]
    let _ = (snap, bytes, replace);
    Ok(())
}

#[cfg(feature = "std")]
fn record_namespace(snap: &mut StoreSnapshot, locale_hash: u64, namespace: &str) {
    let ns_map = snap
        .loaded_namespaces
        .get_or_insert_with(|| Arc::new(HashMap::new()));
    let map = Arc::make_mut(ns_map);
    let list = map.entry(locale_hash).or_insert_with(|| Arc::from([]));
    let mut names: Vec<Arc<str>> = list.iter().cloned().collect();
    if !names.iter().any(|n| n.as_ref() == namespace) {
        names.push(Arc::from(namespace));
        names.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        map.insert(locale_hash, Arc::from(names.into_boxed_slice()));
    }
}

#[cfg(feature = "std")]
pub(crate) fn apply_l10n_to_snapshot(
    snap: &mut StoreSnapshot,
    locale_str: &str,
    bytes: Vec<u8>,
    mode: LoadMode<'_>,
) -> CoreResult<()> {
    crate::binary_format::BinaryFormatReader::new(&bytes)?;
    let locale_hash = crate::binary_format::fnv1a_64(locale_str.as_bytes());

    let (final_bytes, replace_debug, namespace_name) = match mode {
        LoadMode::Replace => {
            lazy_cache_mut(&mut snap.lazy_cache).remove(&locale_hash);
            let ns_map = snap
                .loaded_namespaces
                .get_or_insert_with(|| Arc::new(HashMap::new()));
            Arc::make_mut(ns_map).remove(&locale_hash);
            (bytes, true, None)
        }
        LoadMode::Namespace(namespace) => {
            let existing = snap
                .locales
                .binary_search_by(|(loc, _)| loc.as_str().cmp(locale_str))
                .ok()
                .and_then(|idx| match snap.locales[idx].1.as_ref() {
                    StoreData::Owned(buf) => Some(buf.as_slice()),
                    StoreData::Static(buf, _) => Some(*buf),
                    #[cfg(feature = "std")]
                    StoreData::Lazy(_) => None,
                });
            let merged = match existing {
                Some(prev) => crate::binary_format::merge_l10n_buffers(prev, &bytes)?,
                None => bytes,
            };
            (merged, false, Some(namespace))
        }
    };

    if let Some(namespace) = namespace_name {
        record_namespace(snap, locale_hash, namespace);
    }

    let reader = crate::binary_format::BinaryFormatReader::new(&final_bytes)?;
    offset_maps_mut(&mut snap.offset_maps).insert(locale_hash, Arc::new(reader.to_offsets()));
    merge_debug_keys(snap, &final_bytes, replace_debug)?;

    upsert_locale(
        &mut snap.locales,
        locale_str.to_string(),
        StoreData::Owned(Arc::new(final_bytes)),
    );
    Ok(())
}

/// Loads raw inner `L10N` binary format bytes into the global store for a given locale.
pub fn load_raw_bytes(locale_str: &str, bytes: Vec<u8>) -> bool {
    try_load_raw_bytes(locale_str, bytes).is_ok()
}

/// Loads raw L10N bytes into a scoped store, replacing any existing locale bundle.
#[cfg(feature = "std")]
pub fn try_load_raw_bytes_for_store(
    handle: Option<StoreHandle>,
    locale_str: &str,
    bytes: Vec<u8>,
) -> CoreResult<()> {
    #[cfg(feature = "tracing")]
    let _span = tracing::trace_span!("l10n4x.load_raw_bytes", locale = locale_str).entered();

    crate::metrics::inc_locale_loads();
    update_store_for(handle, |store| {
        let mut snap = store_snapshot(store);
        apply_l10n_to_snapshot(&mut snap, locale_str, bytes, LoadMode::Replace)?;
        Ok(build_store(snap))
    })?;
    notify_locale_changed_for_handle(handle, locale_str);
    Ok(())
}

/// Loads raw L10N bytes, replacing any existing locale bundle (monolith mode).
pub fn try_load_raw_bytes(locale_str: &str, bytes: Vec<u8>) -> CoreResult<()> {
    #[cfg(feature = "std")]
    {
        try_load_raw_bytes_for_store(None, locale_str, bytes)
    }
    #[cfg(not(feature = "std"))]
    {
        let (mut locales, fallback_chain) = read_store(|store| {
            (
                Arc::clone(&store.locales),
                Arc::clone(&store.fallback_chain),
            )
        });
        crate::binary_format::BinaryFormatReader::new(&bytes)?;
        upsert_locale(
            &mut locales,
            locale_str.to_string(),
            StoreData::Owned(Arc::new(bytes)),
        );
        crate::store::swap_store(TranslationStore {
            locales,
            fallback_chain,
        });
        emit_locale_changed(locale_str);
        Ok(())
    }
}

/// Merges a namespace bundle into a scoped store (modular bundle mode).
#[cfg(feature = "std")]
pub fn try_load_namespace_bytes_for_store(
    handle: Option<StoreHandle>,
    locale_str: &str,
    namespace: &str,
    bytes: Vec<u8>,
) -> CoreResult<()> {
    crate::metrics::inc_locale_loads();
    update_store_for(handle, |store| {
        let mut snap = store_snapshot(store);
        apply_l10n_to_snapshot(&mut snap, locale_str, bytes, LoadMode::Namespace(namespace))?;
        Ok(build_store(snap))
    })?;
    notify_locale_changed_for_handle(handle, locale_str);
    Ok(())
}

/// Merges a namespace bundle into an existing locale (modular bundle mode).
#[cfg(feature = "std")]
pub fn try_load_namespace_bytes(
    locale_str: &str,
    namespace: &str,
    bytes: Vec<u8>,
) -> CoreResult<()> {
    try_load_namespace_bytes_for_store(None, locale_str, namespace, bytes)
}

/// Merges a signed namespace `.pak` into a scoped store.
#[cfg(feature = "std")]
pub fn try_load_namespace_pak_for_store(
    handle: Option<StoreHandle>,
    locale_str: &str,
    namespace: &str,
    pak_bytes: &[u8],
) -> CoreResult<()> {
    let decompressed = decompress_pak(pak_bytes)?;
    try_load_namespace_bytes_for_store(handle, locale_str, namespace, decompressed)
}

/// Merges a signed namespace `.pak` into `locale_str`.
#[cfg(feature = "std")]
pub fn try_load_namespace_pak(
    locale_str: &str,
    namespace: &str,
    pak_bytes: &[u8],
) -> CoreResult<()> {
    try_load_namespace_pak_for_store(None, locale_str, namespace, pak_bytes)
}

/// Merges a signed namespace `.pak` from raw bytes; returns `true` on success.
#[cfg(feature = "std")]
pub fn load_namespace_pak(locale_str: &str, namespace: &str, pak_bytes: &[u8]) -> bool {
    try_load_namespace_pak(locale_str, namespace, pak_bytes).is_ok()
}

/// Loads a namespace `.pak` from disk into a scoped store.
#[cfg(feature = "std")]
pub fn try_load_namespace_locale_for_store(
    handle: Option<StoreHandle>,
    locale_str: &str,
    namespace: &str,
    path_str: &str,
) -> CoreResult<()> {
    let bytes = std::fs::read(path_str).map_err(|e| {
        // CoreError carries only static strings (no_std-compatible); log the
        // real cause instead of discarding it.
        log::warn!("l10n4x: failed to read pak '{path_str}': {e}");
        crate::CoreError::IoError("read failed")
    })?;
    try_load_namespace_pak_for_store(handle, locale_str, namespace, &bytes)
}

/// Loads a namespace `.pak` from disk into `locale_str`.
#[cfg(feature = "std")]
pub fn try_load_namespace_locale(
    locale_str: &str,
    namespace: &str,
    path_str: &str,
) -> CoreResult<()> {
    try_load_namespace_locale_for_store(None, locale_str, namespace, path_str)
}

/// Loads a namespace `.pak` from disk; returns `true` on success.
#[cfg(feature = "std")]
pub fn load_namespace_locale(locale_str: &str, namespace: &str, path_str: &str) -> bool {
    try_load_namespace_locale(locale_str, namespace, path_str).is_ok()
}

/// Loads preload namespaces listed in `namespaces.json` under `base_dir`.
#[cfg(feature = "std")]
pub fn init_modular(base_dir: &str, locale: &str) -> CoreResult<()> {
    let manifest_path = std::path::Path::new(base_dir).join("namespaces.json");
    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|_| crate::CoreError::IoError("namespaces.json read failed"))?;
    let manifest: NamespaceManifest = serde_json::from_str(&raw)
        .map_err(|_| crate::CoreError::InvalidFormat("invalid namespaces.json"))?;
    let preload = manifest
        .preload
        .or(manifest.locales.get(locale).cloned())
        .unwrap_or_default();
    for ns in preload {
        let path = std::path::Path::new(base_dir)
            .join(locale)
            .join(format!("{ns}.pak"));
        let path_str = path
            .to_str()
            .ok_or(crate::CoreError::InvalidFormat("invalid pak path"))?;
        try_load_namespace_locale(locale, &ns, path_str)?;
    }
    Ok(())
}

/// Manifest emitted by `l10n4x build` in modular bundle mode.
#[cfg(feature = "std")]
#[derive(serde::Deserialize)]
pub struct NamespaceManifest {
    /// Global preload list (overridden per locale when `locales` entry exists).
    #[serde(default)]
    pub preload: Option<Vec<String>>,
    /// Per-locale namespace lists available for lazy loading.
    #[serde(default)]
    pub locales: HashMap<String, Vec<String>>,
}

/// Decompresses and loads a single `.pak` file from raw bytes for a given locale.
pub fn load_pak_bytes(locale_str: &str, pak_bytes: &[u8]) -> bool {
    try_load_pak_bytes(locale_str, pak_bytes).is_ok()
}

/// Decompresses and loads a `.pak` into a scoped store, replacing any existing bundle.
#[cfg(feature = "std")]
pub fn try_load_pak_bytes_for_store(
    handle: Option<StoreHandle>,
    locale_str: &str,
    pak_bytes: &[u8],
) -> CoreResult<()> {
    let decompressed = decompress_pak(pak_bytes)?;
    try_load_raw_bytes_for_store(handle, locale_str, decompressed)
}

/// Decompresses and loads a `.pak` for `locale_str`, replacing any existing bundle.
pub fn try_load_pak_bytes(locale_str: &str, pak_bytes: &[u8]) -> CoreResult<()> {
    let decompressed = decompress_pak(pak_bytes)?;
    #[cfg(feature = "std")]
    {
        try_load_raw_bytes_for_store(None, locale_str, decompressed)
    }
    #[cfg(not(feature = "std"))]
    {
        try_load_raw_bytes(locale_str, decompressed)
    }
}

/// Stores compressed `.pak` bytes for lazy decompression on first lookup.
#[cfg(feature = "std")]
pub fn load_pak_lazy(locale_str: &str, pak_bytes: &[u8]) -> bool {
    try_load_pak_lazy(locale_str, pak_bytes).is_ok()
}

/// Verifies signature and stores compressed bytes for lazy decompression.
#[cfg(feature = "std")]
pub fn try_load_pak_lazy(locale_str: &str, pak_bytes: &[u8]) -> CoreResult<()> {
    crate::metrics::inc_locale_loads();
    let signed = crate::envelope::open_outer(pak_bytes)?;
    let (message, compressed, signature, _parent) = crate::pak::parse_signed(&signed)?;
    crate::integrity::verify(message, signature)?;
    crate::store::update_store::<_, crate::CoreError>(|store| {
        let mut snap = store_snapshot(store);
        let lazy = lazy_cache_mut(&mut snap.lazy_cache);
        let offset_map = offset_maps_mut(&mut snap.offset_maps);
        let locale_hash = crate::binary_format::fnv1a_64(locale_str.as_bytes());
        let had_locale = snap
            .locales
            .binary_search_by(|(loc, _)| loc.as_str().cmp(locale_str))
            .is_ok();
        if had_locale {
            lazy.remove(&locale_hash);
            offset_map.remove(&locale_hash);
        }
        lazy.entry(locale_hash)
            .or_insert_with(|| Arc::new(OnceLock::new()));
        offset_map
            .entry(locale_hash)
            .or_insert_with(|| Arc::new(HashMap::new()));
        upsert_locale(
            &mut snap.locales,
            locale_str.to_string(),
            StoreData::Lazy(Arc::new(Vec::from(compressed))),
        );
        Ok(build_store(snap))
    })?;
    emit_locale_changed(locale_str);
    Ok(())
}

/// Loads a monolith `{locale}.pak` from disk; returns `true` on success.
#[cfg(feature = "std")]
pub fn load_pak_locale(locale_str: &str, path_str: &str) -> bool {
    try_load_pak_locale(locale_str, path_str).is_ok()
}

/// Loads a monolith `.pak` from disk into a scoped store.
#[cfg(feature = "std")]
pub fn try_load_pak_locale_for_store(
    handle: Option<StoreHandle>,
    locale_str: &str,
    path_str: &str,
) -> CoreResult<()> {
    let bytes = std::fs::read(path_str).map_err(|e| {
        // CoreError carries only static strings (no_std-compatible); log the
        // real cause instead of discarding it.
        log::warn!("l10n4x: failed to read pak '{path_str}': {e}");
        crate::CoreError::IoError("read failed")
    })?;
    try_load_pak_bytes_for_store(handle, locale_str, &bytes)
}

/// Loads a monolith `.pak` from disk, replacing the locale bundle.
#[cfg(feature = "std")]
pub fn try_load_pak_locale(locale_str: &str, path_str: &str) -> CoreResult<()> {
    try_load_pak_locale_for_store(None, locale_str, path_str)
}

/// Loads compile-time embedded L10N bytes; returns `true` on success.
pub fn load_static_bytes(locale_str: &str, data: &'static [u8], already_verified: bool) -> bool {
    try_load_static_bytes(locale_str, data, already_verified).is_ok()
}

/// Loads L10N bytes into a scoped store (copies into owned storage).
#[cfg(feature = "std")]
pub fn try_load_static_bytes_for_store(
    handle: Option<StoreHandle>,
    locale_str: &str,
    data: &[u8],
    already_verified: bool,
) -> CoreResult<()> {
    let _ = already_verified;
    crate::binary_format::BinaryFormatReader::new(data)?;
    try_load_raw_bytes_for_store(handle, locale_str, data.to_vec())
}

/// Loads compile-time embedded L10N bytes into the store.
pub fn try_load_static_bytes(
    locale_str: &str,
    data: &'static [u8],
    already_verified: bool,
) -> CoreResult<()> {
    crate::binary_format::BinaryFormatReader::new(data)?;
    if crate::store::load_static_bytes(locale_str, data, already_verified) {
        Ok(())
    } else {
        Err(crate::CoreError::IoError("failed to load static bytes"))
    }
}

/// Scans a directory for monolith `{locale}.pak` files (legacy layout).
#[cfg(feature = "std")]
pub fn load_pak_directory(dir_path_str: &str) -> bool {
    let path = std::path::Path::new(dir_path_str);
    if !path.is_dir() {
        return false;
    }
    let entries = match std::fs::read_dir(path) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let mut loaded_any = false;
    for entry in entries.flatten() {
        let file_path = entry.path();
        if file_path.is_file() && file_path.extension().is_some_and(|ext| ext == "pak") {
            if let Some(locale) = file_path.file_stem().and_then(|s| s.to_str()) {
                if let Some(path_str) = file_path.to_str() {
                    if load_pak_locale(locale, path_str) {
                        loaded_any = true;
                    }
                }
            }
        }
    }
    loaded_any
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binary_format::{fnv1a_64, merge_l10n_buffers, pack_l10n, RUNTIME_VERSION};
    use crate::store::{clear_translations, locale_loaded, namespace_loaded};
    use alloc::vec::Vec;

    #[cfg(feature = "std")]
    static LOADER_TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[cfg(feature = "std")]
    fn loader_test_lock() -> std::sync::MutexGuard<'static, ()> {
        LOADER_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn make_l10n_with_key(key: &str, val: &[u8]) -> Vec<u8> {
        let entries: Vec<(u64, Vec<u8>)> = vec![(fnv1a_64(key.as_bytes()), val.to_vec())];
        pack_l10n(
            &entries,
            RUNTIME_VERSION,
            crate::locale_data::LOCALE_DATA_VERSION,
            None,
        )
    }

    #[test]
    fn load_raw_bytes_success() {
        #[cfg(feature = "std")]
        let _lock = loader_test_lock();
        clear_translations();
        assert!(load_raw_bytes(
            "test",
            pack_l10n::<Vec<u8>>(
                &[],
                RUNTIME_VERSION,
                crate::locale_data::LOCALE_DATA_VERSION,
                None,
            )
        ));
    }

    #[test]
    fn load_namespace_merges_keys() {
        #[cfg(feature = "std")]
        let _lock = loader_test_lock();
        clear_translations();
        let common = make_l10n_with_key("common.welcome", b"hi");
        let auth = make_l10n_with_key("auth.login", b"login");
        assert!(load_raw_bytes("en", common));
        assert!(try_load_namespace_bytes("en", "auth", auth).is_ok());
        assert!(namespace_loaded("en", "auth"));
        assert!(locale_loaded("en"));
    }

    #[test]
    fn load_raw_bytes_overwrites_monolith_namespaces() {
        #[cfg(feature = "std")]
        let _lock = loader_test_lock();
        clear_translations();
        let a = make_l10n_with_key("a", b"1");
        let b = make_l10n_with_key("b", b"2");
        try_load_namespace_bytes("en", "auth", a).unwrap();
        assert!(namespace_loaded("en", "auth"));
        load_raw_bytes("en", b);
        assert!(!namespace_loaded("en", "auth"));
    }

    #[test]
    fn merge_l10n_preserves_both_keys() {
        let a = make_l10n_with_key("a", b"1");
        let b = make_l10n_with_key("b", b"2");
        let merged = merge_l10n_buffers(&a, &b).unwrap();
        let reader = crate::binary_format::BinaryFormatReader::new(&merged).unwrap();
        assert!(reader.lookup(fnv1a_64(b"a")).is_some());
        assert!(reader.lookup(fnv1a_64(b"b")).is_some());
    }
}
