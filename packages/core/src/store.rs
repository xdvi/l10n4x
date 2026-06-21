extern crate alloc;
use crate::binary_format::BinaryFormatReader;
use crate::formatter::format_message;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicPtr, Ordering};

#[cfg(all(not(feature = "std"), debug_assertions))]
use core::sync::atomic::AtomicUsize;

/// Manages loaded localization packages containing locales and their decompressed binary buffers.
pub struct TranslationStore {
    /// List of loaded locale-to-buffer mappings.
    pub locales: Vec<(String, Arc<Vec<u8>>)>,
    /// Fallback locale (defaults to "en").
    pub fallback: String,
}

impl Default for TranslationStore {
    fn default() -> Self {
        Self {
            locales: Vec::new(),
            fallback: "en".to_string(),
        }
    }
}

impl TranslationStore {
    /// Looks up the decompressed translation package buffer for a given locale.
    /// Returns `Some(&[u8])` if loaded, or `None` otherwise.
    pub fn lookup(&self, locale: &str) -> Option<&[u8]> {
        for (loc, buf) in &self.locales {
            if loc == locale {
                return Some(buf.as_slice());
            }
        }
        None
    }
}

// Global store pointer
static STORE: AtomicPtr<TranslationStore> = AtomicPtr::new(core::ptr::null_mut());

#[cfg(all(not(feature = "std"), debug_assertions))]
static REENTRANCY_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(all(not(feature = "std"), debug_assertions))]
struct ReentrancyGuard;

#[cfg(all(not(feature = "std"), debug_assertions))]
impl ReentrancyGuard {
    fn new() -> Self {
        REENTRANCY_COUNTER.fetch_add(1, Ordering::SeqCst);
        ReentrancyGuard
    }
}

#[cfg(all(not(feature = "std"), debug_assertions))]
impl Drop for ReentrancyGuard {
    fn drop(&mut self) {
        REENTRANCY_COUNTER.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Safely executes a function with a reference to the current TranslationStore
#[cfg(feature = "std")]
pub fn read_store<F, R>(f: F) -> R
where
    F: FnOnce(&TranslationStore) -> R,
{
    let _guard = crossbeam_epoch::pin();
    let ptr = STORE.load(Ordering::Acquire);
    if ptr.is_null() {
        let empty = TranslationStore::default();
        f(&empty)
    } else {
        // SAFETY: Pointer is loaded within the epoch guard, which guarantees that the
        // memory pointed to by `ptr` will not be reclaimed until the guard is dropped.
        unsafe { f(&*ptr) }
    }
}

/// Safely executes a function with a reference to the current TranslationStore
#[cfg(not(feature = "std"))]
pub fn read_store<F, R>(f: F) -> R
where
    F: FnOnce(&TranslationStore) -> R,
{
    #[cfg(debug_assertions)]
    let _guard = ReentrancyGuard::new();

    let ptr = STORE.load(Ordering::Acquire);
    if ptr.is_null() {
        let empty = TranslationStore::default();
        f(&empty)
    } else {
        // SAFETY: In a single-threaded environment, there are no concurrent mutations
        // or preemption, so referencing the store is safe.
        unsafe { f(&*ptr) }
    }
}

/// Swaps the current store with a new one and schedules the old store for reclamation
#[cfg(feature = "std")]
pub fn swap_store(new_store: TranslationStore) {
    let new_ptr = Box::into_raw(Box::new(new_store));
    let old_ptr = STORE.swap(new_ptr, Ordering::SeqCst);
    if !old_ptr.is_null() {
        crate::reclaim::schedule_drop(old_ptr);
    }
}

/// Swaps the current store with a new one and schedules the old store for reclamation
#[cfg(not(feature = "std"))]
pub fn swap_store(new_store: TranslationStore) {
    #[cfg(debug_assertions)]
    assert_eq!(
        REENTRANCY_COUNTER.load(Ordering::Acquire),
        0,
        "Reentrancy detected: swap_store called within read_store"
    );

    let new_ptr = Box::into_raw(Box::new(new_store));
    let old_ptr = STORE.swap(new_ptr, Ordering::SeqCst);
    if !old_ptr.is_null() {
        crate::reclaim::schedule_drop(old_ptr);
    }
}

/// Reclaims memory for any retired stores (no-op, kept for API backwards compatibility)
pub fn try_reclaim() {}

/// Returns the currently configured fallback locale (defaults to "en").
pub fn get_fallback_locale() -> String {
    read_store(|store| store.fallback.clone())
}

/// Sets the global fallback locale (defaults to "en").
pub fn set_fallback_locale(locale_str: &str) {
    read_store(|store| {
        let new_store = TranslationStore {
            locales: store.locales.clone(),
            fallback: locale_str.to_string(),
        };
        swap_store(new_store);
    });
}

/// Clears all loaded translations.
pub fn clear_translations() {
    read_store(|store| {
        swap_store(TranslationStore {
            locales: Vec::new(),
            fallback: store.fallback.clone(),
        });
    });
}

/// Returns `true` if `key` exists in `locale` or the configured fallback locale.
pub fn key_exists(locale: &str, key: &str) -> bool {
    let fallback = get_fallback_locale();
    read_store(|store| {
        for loc in [locale, fallback.as_str()] {
            if let Some(buf) = store.lookup(loc) {
                if let Ok(reader) = BinaryFormatReader::new(buf) {
                    if reader.lookup(key).is_some() {
                        return true;
                    }
                }
            }
        }
        false
    })
}

/// Returns `true` if translations for `locale` are loaded.
pub fn locale_loaded(locale: &str) -> bool {
    read_store(|store| store.lookup(locale).is_some())
}

/// Helper function to translate a key directly into a caller-provided Writer
pub fn translate_to_writer<W: core::fmt::Write>(
    locale: &str,
    key: &str,
    params: &[(&str, &str)],
    writer: &mut W,
) -> Result<(), &'static str> {
    let fallback = get_fallback_locale();

    let success = read_store(|store| {
        if let Some(buf) = store.lookup(locale) {
            if let Ok(reader) = BinaryFormatReader::new(buf) {
                if let Some(bytecode) = reader.lookup(key) {
                    if format_message(bytecode, locale, params, writer).is_ok() {
                        return Some(());
                    }
                }
            }
        }
        if locale != fallback {
            if let Some(buf) = store.lookup(&fallback) {
                if let Ok(reader) = BinaryFormatReader::new(buf) {
                    if let Some(bytecode) = reader.lookup(key) {
                        if format_message(bytecode, &fallback, params, writer).is_ok() {
                            return Some(());
                        }
                    }
                }
            }
        }
        None
    });

    if success.is_some() {
        Ok(())
    } else {
        writer
            .write_str(key)
            .map_err(|_| "Failed to write key fallback")?;
        Ok(())
    }
}

/// Translates a key for a given locale, dynamically interpolating parameters,
/// and returning an allocated String.
pub fn translate(locale: &str, key: &str, params: &[(&str, &str)]) -> String {
    let mut buf = String::new();
    let _ = translate_to_writer(locale, key, params, &mut buf);
    buf
}
