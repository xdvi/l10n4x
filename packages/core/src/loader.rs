extern crate alloc;
use crate::error::CoreResult;
use crate::pak::decompress_pak;
use crate::store::{emit_locale_changed, read_store, swap_store, StoreData, TranslationStore};
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::vec::Vec;

#[cfg(feature = "std")]
use std::collections::HashMap;
#[cfg(feature = "std")]
use std::sync::OnceLock;

/// Loads raw inner `L10N` binary format bytes into the global store for a given locale.
/// Takes ownership of `bytes` to avoid an extra allocation (caller typically has a `Vec<u8>`
/// from `decompress_pak`).
pub fn load_raw_bytes(locale_str: &str, bytes: Vec<u8>) -> bool {
    crate::metrics::inc_locale_loads();
    #[cfg(feature = "std")]
    {
        let (mut locales, fallback_chain, mut _lazy_cache, mut offset_maps) = read_store(|store| {
            (
                Arc::clone(&store.locales),
                Arc::clone(&store.fallback_chain),
                store.lazy_cache.clone(),
                store.offset_maps.clone(),
            )
        });
        let new_vec = Arc::make_mut(&mut locales);
        let locale_hash = crate::binary_format::fnv1a_64(locale_str.as_bytes());
        let offset_arc = if let Ok(reader) = crate::binary_format::BinaryFormatReader::new(&bytes) {
            Arc::new(reader.to_offsets())
        } else {
            Arc::new(HashMap::new())
        };
        let entry = (locale_str.to_string(), StoreData::Owned(Arc::new(bytes)));
        match new_vec.binary_search_by(|(loc, _)| loc.as_str().cmp(locale_str)) {
            Ok(pos) => {
                _lazy_cache.remove(&locale_hash);
                offset_maps.insert(locale_hash, offset_arc);
                new_vec[pos] = entry;
            }
            Err(pos) => {
                offset_maps.insert(locale_hash, offset_arc);
                new_vec.insert(pos, entry);
            }
        }
        swap_store(TranslationStore {
            locales,
            fallback_chain,
            lazy_cache: _lazy_cache,
            offset_maps,
        });
        emit_locale_changed(locale_str);
        true
    }
    #[cfg(not(feature = "std"))]
    {
        let (mut locales, fallback_chain) =
            read_store(|store| (Arc::clone(&store.locales), Arc::clone(&store.fallback_chain)));
        let new_vec = Arc::make_mut(&mut locales);
        let entry = (locale_str.to_string(), StoreData::Owned(Arc::new(bytes)));
        match new_vec.binary_search_by(|(loc, _)| loc.as_str().cmp(locale_str)) {
            Ok(pos) => new_vec[pos] = entry,
            Err(pos) => new_vec.insert(pos, entry),
        }
        swap_store(TranslationStore {
            locales,
            fallback_chain,
        });
        emit_locale_changed(locale_str);
        true
    }
}

/// Decompresses and loads a single `.pak` file from raw bytes for a given locale.
pub fn load_pak_bytes(locale_str: &str, pak_bytes: &[u8]) -> bool {
    try_load_pak_bytes(locale_str, pak_bytes).is_ok()
}

/// Decompresses and loads a single `.pak` file from raw bytes for a given locale.
pub fn try_load_pak_bytes(locale_str: &str, pak_bytes: &[u8]) -> CoreResult<()> {
    let decompressed = decompress_pak(pak_bytes)?;
    if load_raw_bytes(locale_str, decompressed) {
        Ok(())
    } else {
        Err(crate::CoreError::IoError(
            "failed to load decompressed bytes",
        ))
    }
}

/// Verifies signature and stores the zstd payload without decompressing.
/// Decompression is deferred until the first `translate` call for the locale.
/// Only available under `feature = "std"`.
#[cfg(feature = "std")]
pub fn load_pak_lazy(locale_str: &str, pak_bytes: &[u8]) -> bool {
    try_load_pak_lazy(locale_str, pak_bytes).is_ok()
}

/// Verifies signature and stores the zstd payload without decompressing.
#[cfg(feature = "std")]
pub fn try_load_pak_lazy(locale_str: &str, pak_bytes: &[u8]) -> CoreResult<()> {
    crate::metrics::inc_locale_loads();
    let signed = crate::envelope::open_outer(pak_bytes)?;
    let (message, compressed, signature, _parent) = crate::pak::parse_signed(&signed)?;
    crate::integrity::verify(message, signature)?;
    let (mut locales, fallback_chain, mut lazy_cache, mut offset_maps) = read_store(|store| {
        (
            Arc::clone(&store.locales),
            Arc::clone(&store.fallback_chain),
            store.lazy_cache.clone(),
            store.offset_maps.clone(),
        )
    });
    let new_vec = Arc::make_mut(&mut locales);
    let locale_hash = crate::binary_format::fnv1a_64(locale_str.as_bytes());
    let entry = (
        locale_str.to_string(),
        StoreData::Lazy(Arc::new(Vec::from(compressed))),
    );
    match new_vec.binary_search_by(|(loc, _)| loc.as_str().cmp(locale_str)) {
        Ok(pos) => {
            lazy_cache.remove(&locale_hash);
            offset_maps.remove(&locale_hash);
            lazy_cache.insert(locale_hash, Arc::new(OnceLock::new()));
            offset_maps.insert(locale_hash, Arc::new(HashMap::new()));
            new_vec[pos] = entry;
        }
        Err(pos) => {
            lazy_cache
                .entry(locale_hash)
                .or_insert_with(|| Arc::new(OnceLock::new()));
            offset_maps
                .entry(locale_hash)
                .or_insert_with(|| Arc::new(HashMap::new()));
            new_vec.insert(pos, entry);
        }
    }
    swap_store(TranslationStore {
        locales,
        fallback_chain,
        lazy_cache,
        offset_maps,
    });
    emit_locale_changed(locale_str);
    Ok(())
}

/// Decompresses and loads a single `.pak` file for a given locale (requires std).
#[cfg(feature = "std")]
pub fn load_pak_locale(locale_str: &str, path_str: &str) -> bool {
    try_load_pak_locale(locale_str, path_str).is_ok()
}

/// Decompresses and loads a single `.pak` file for a given locale (requires std).
#[cfg(feature = "std")]
pub fn try_load_pak_locale(locale_str: &str, path_str: &str) -> CoreResult<()> {
    let bytes = std::fs::read(path_str).map_err(|_| crate::CoreError::IoError("read failed"))?;
    try_load_pak_bytes(locale_str, &bytes)
}

/// Convenience wrapper around [`crate::store::load_static_bytes`].
pub fn load_static_bytes(locale_str: &str, data: &'static [u8], already_verified: bool) -> bool {
    try_load_static_bytes(locale_str, data, already_verified).is_ok()
}

/// Loads a static (compile-time embedded) L10N buffer into the global store.
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

/// Scans a directory for all `.pak` files and automatically loads them (requires std).
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
    use crate::store::{clear_translations, locale_loaded};
    use alloc::vec::Vec;

    fn make_l10n_bytes() -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&16u32.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf
    }

    #[test]
    fn load_raw_bytes_success() {
        clear_translations();
        let bytes = make_l10n_bytes();
        assert!(load_raw_bytes("test", bytes));
    }

    #[test]
    fn load_raw_bytes_then_locale_loaded() {
        clear_translations();
        let bytes = make_l10n_bytes();
        load_raw_bytes("test-loc", bytes);
        assert!(locale_loaded("test-loc"));
    }

    #[test]
    fn load_raw_bytes_overwrites_existing_locale() {
        clear_translations();
        let bytes1 = make_l10n_bytes();
        let mut bytes2 = make_l10n_bytes();
        bytes2.push(0xFF);
        assert!(load_raw_bytes("dup", bytes1));
        assert!(load_raw_bytes("dup", bytes2));
    }

    #[test]
    fn load_pak_bytes_invalid_fails() {
        let result = load_pak_bytes("xx", b"not a pak");
        assert!(!result);
    }

    #[test]
    fn try_load_pak_bytes_invalid_returns_err() {
        let result = try_load_pak_bytes("xx", b"not a pak");
        assert!(result.is_err());
    }

    #[test]
    fn load_pak_locale_nonexistent_file() {
        let result = load_pak_locale("xx", "/nonexistent/path.pak");
        assert!(!result);
    }

    #[test]
    fn load_pak_directory_rejects_file_path() {
        let result = load_pak_directory("/dev/null");
        assert!(!result);
    }
}
