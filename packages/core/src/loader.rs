extern crate alloc;
use alloc::string::ToString;
use crate::crypto::decrypt_gcm;
use crate::store::{read_store, swap_store, TranslationStore};

/// Loads raw (decrypted) binary format bytes into the global store for a given locale.
pub fn load_raw_bytes(locale_str: &str, bytes: &[u8]) -> bool {
    let mut success = false;
    read_store(|store| {
        let mut new_locales = store.locales.clone();
        if let Some(pos) = new_locales.iter().position(|(loc, _)| loc == locale_str) {
            new_locales[pos] = (locale_str.to_string(), bytes.to_vec());
        } else {
            new_locales.push((locale_str.to_string(), bytes.to_vec()));
        }
        swap_store(TranslationStore { locales: new_locales });
        success = true;
    });
    success
}

/// Decrypts and loads a single .pak file from raw bytes for a given locale.
pub fn load_pak_bytes(locale_str: &str, encrypted_bytes: &[u8]) -> bool {
    if let Ok(decrypted) = decrypt_gcm(encrypted_bytes) {
        if let Ok(decompressed) = miniz_oxide::inflate::decompress_to_vec(&decrypted) {
            load_raw_bytes(locale_str, &decompressed)
        } else {
            false
        }
    } else {
        false
    }
}

/// Decrypts and loads a single .pak file for a given locale (requires std).
#[cfg(feature = "std")]
pub fn load_pak_locale(locale_str: &str, path_str: &str) -> bool {
    if let Ok(bytes) = std::fs::read(path_str) {
        load_pak_bytes(locale_str, &bytes)
    } else {
        false
    }
}

/// Scans a directory for all .pak files and automatically loads them (requires std).
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
    for entry in entries {
        if let Ok(entry) = entry {
            let file_path = entry.path();
            if file_path.is_file() && file_path.extension().map_or(false, |ext| ext == "pak") {
                if let Some(locale) = file_path.file_stem().and_then(|s| s.to_str()) {
                    if let Some(path_str) = file_path.to_str() {
                        if load_pak_locale(locale, path_str) {
                            loaded_any = true;
                        }
                    }
                }
            }
        }
    }

    loaded_any
}
