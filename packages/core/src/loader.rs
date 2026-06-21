extern crate alloc;
use crate::pak::decompress_pak;
use crate::store::{read_store, swap_store, TranslationStore};
use alloc::string::ToString;
use alloc::sync::Arc;

/// Loads raw inner `L10N` binary format bytes into the global store for a given locale.
pub fn load_raw_bytes(locale_str: &str, bytes: &[u8]) -> bool {
    let mut success = false;
    read_store(|store| {
        let mut new_vec = (*store.locales).clone();
        let entry = (locale_str.to_string(), Arc::new(bytes.to_vec()));
        match new_vec.binary_search_by(|(loc, _)| loc.as_str().cmp(locale_str)) {
            Ok(pos) => new_vec[pos] = entry,
            Err(pos) => new_vec.insert(pos, entry),
        }
        swap_store(TranslationStore {
            locales: Arc::new(new_vec),
            fallback_chain: alloc::sync::Arc::clone(&store.fallback_chain),
        });
        success = true;
    });
    success
}

/// Decompresses and loads a single `.pak` file from raw bytes for a given locale.
pub fn load_pak_bytes(locale_str: &str, pak_bytes: &[u8]) -> bool {
    match decompress_pak(pak_bytes) {
        Ok(decompressed) => load_raw_bytes(locale_str, &decompressed),
        Err(_) => false,
    }
}

/// Decompresses and loads a single `.pak` file for a given locale (requires std).
#[cfg(feature = "std")]
pub fn load_pak_locale(locale_str: &str, path_str: &str) -> bool {
    if let Ok(bytes) = std::fs::read(path_str) {
        load_pak_bytes(locale_str, &bytes)
    } else {
        false
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
