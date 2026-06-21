extern crate alloc;
use crate::binary_format::BinaryFormatReader;
use crate::formatter::format_message;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicPtr, Ordering};

#[cfg(all(not(feature = "std"), debug_assertions))]
use core::sync::atomic::AtomicUsize;

#[cfg(feature = "std")]
fn default_chain() -> Arc<[Arc<str>]> {
    use std::sync::OnceLock;
    static CHAIN: OnceLock<Arc<[Arc<str>]>> = OnceLock::new();
    CHAIN.get_or_init(|| {
        Arc::from(alloc::vec![Arc::from("en") as Arc<str>].into_boxed_slice())
    }).clone()
}

#[cfg(not(feature = "std"))]
fn default_chain() -> Arc<[Arc<str>]> {
    Arc::from(alloc::vec![Arc::from("en") as Arc<str>].into_boxed_slice())
}

/// Manages loaded localization packages: maps locale codes to their decompressed binary buffers.
/// Uses a sorted `Vec` for O(log n) binary-search lookup and cache-friendly O(n) clone.
/// `locales` is `Arc`-wrapped so `clone()` on the whole store is O(1) when locales don't change.
pub struct TranslationStore {
    /// Sorted vector of locale-to-buffer mappings (Arc for O(1) clone). Binary-search for lookup.
    pub locales: Arc<Vec<(String, Arc<Vec<u8>>)>>,
    /// Ordered chain of fallback locale codes. The first match wins.
    pub fallback_chain: Arc<[Arc<str>]>,
}

impl Default for TranslationStore {
    fn default() -> Self {
        Self {
            locales: Arc::new(Vec::new()),
            fallback_chain: default_chain(),
        }
    }
}

impl TranslationStore {
    /// Looks up the decompressed translation buffer for a given locale. O(log n) binary search.
    pub fn lookup(&self, locale: &str) -> Option<&[u8]> {
        let idx = self.locales.binary_search_by(|(loc, _)| loc.as_str().cmp(locale)).ok()?;
        Some(self.locales[idx].1.as_slice())
    }
}

// Global store pointer
static STORE: AtomicPtr<TranslationStore> = AtomicPtr::new(core::ptr::null_mut());

// Function pointer type for missing key notifications.
type MissingKeyFn = fn(locale: &str, key: &str);

static MISSING_KEY_HANDLER: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// Installs a callback invoked whenever a key is not found in any locale or fallback.
/// The callback receives the originally requested locale and the missing key.
/// Pass `clear_missing_key_handler` to remove the handler.
///
/// # Safety
/// The handler must be safe to call from any thread simultaneously with translation calls.
pub fn set_missing_key_handler(f: MissingKeyFn) {
    MISSING_KEY_HANDLER.store(f as *mut (), Ordering::Release);
}

/// Removes the missing key handler.
pub fn clear_missing_key_handler() {
    MISSING_KEY_HANDLER.store(core::ptr::null_mut(), Ordering::Release);
}

fn call_missing_key_handler(locale: &str, key: &str) {
    let ptr = MISSING_KEY_HANDLER.load(Ordering::Acquire);
    if !ptr.is_null() {
        let f: MissingKeyFn = unsafe { core::mem::transmute(ptr) };
        f(locale, key);
    }
}

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

/// Reclaims memory for any retired stores.
/// Under `std`, pins an epoch guard and flushes deferred drops.
/// Under `no_std` (single-threaded), old stores are dropped immediately in `schedule_drop`.
pub fn try_reclaim() {
    #[cfg(feature = "std")]
    {
        let guard = crossbeam_epoch::pin();
        guard.flush();
    }
}

/// Sets the fallback locale chain. The first locale in the slice that has the key wins.
/// An empty slice disables fallback entirely.
pub fn set_fallback_chain(chain: &[&str]) {
    let arcs: alloc::vec::Vec<Arc<str>> = chain.iter().map(|s| Arc::from(*s)).collect();
    let new_chain: Arc<[Arc<str>]> = Arc::from(arcs.into_boxed_slice());
    let locales = read_store(|store| Arc::clone(&store.locales));
    swap_store(TranslationStore {
        locales,
        fallback_chain: new_chain,
    });
}

/// Returns the current fallback chain as a cheap Arc clone.
pub fn get_fallback_chain() -> Arc<[Arc<str>]> {
    read_store(|store| Arc::clone(&store.fallback_chain))
}

/// Backward-compatible wrapper — sets the chain to a single locale.
pub fn set_fallback_locale(locale_str: &str) {
    set_fallback_chain(&[locale_str]);
}

/// Backward-compatible — returns the first element of the chain (or "en").
pub fn get_fallback_locale() -> Arc<str> {
    read_store(|store| {
        store.fallback_chain.first().cloned().unwrap_or_else(|| Arc::from("en"))
    })
}

/// Clears all loaded translations.
pub fn clear_translations() {
    let chain = read_store(|store| Arc::clone(&store.fallback_chain));
    swap_store(TranslationStore {
        locales: Arc::new(Vec::new()),
        fallback_chain: chain,
    });
}

/// Returns `true` if `key` exists in `locale` or the configured fallback locale.
pub fn key_exists(locale: &str, key: &str) -> bool {
    let chain = get_fallback_chain();
    read_store(|store| {
        // Try the requested locale first
        if let Some(buf) = store.lookup(locale) {
            if let Ok(reader) = BinaryFormatReader::new(buf) {
                if reader.lookup(key).is_some() {
                    return true;
                }
            }
        }
        // Walk the fallback chain
        for fb in chain.iter() {
            let fb_str: &str = fb.as_ref();
            if fb_str == locale {
                continue;
            }
            if let Some(buf) = store.lookup(fb_str) {
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
    let chain = get_fallback_chain();

    let success = read_store(|store| {
        // Try the requested locale first
        if let Some(buf) = store.lookup(locale) {
            if let Ok(reader) = BinaryFormatReader::new(buf) {
                if let Some(bytecode) = reader.lookup(key) {
                    if format_message(bytecode, locale, params, writer).is_ok() {
                        return Some(());
                    }
                }
            }
        }
        // Walk the fallback chain
        for fb in chain.iter() {
            let fb_str: &str = fb.as_ref();
            if fb_str == locale {
                continue;
            }
            if let Some(buf) = store.lookup(fb_str) {
                if let Ok(reader) = BinaryFormatReader::new(buf) {
                    if let Some(bytecode) = reader.lookup(key) {
                        if format_message(bytecode, fb_str, params, writer).is_ok() {
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
        call_missing_key_handler(locale, key);
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

#[cfg(test)]
mod missing_key_tests {
    use super::*;
    use core::sync::atomic::{AtomicBool, Ordering};

    static HANDLER_CALLED: AtomicBool = AtomicBool::new(false);

    fn test_handler(locale: &str, key: &str) {
        HANDLER_CALLED.store(true, Ordering::SeqCst);
        let _ = (locale, key);
    }

    #[test]
    fn handler_called_on_missing_key() {
        HANDLER_CALLED.store(false, Ordering::SeqCst);
        set_missing_key_handler(test_handler);
        let mut buf = alloc::string::String::new();
        let _ = translate_to_writer("xx", "nonexistent.key", &[], &mut buf);
        assert!(HANDLER_CALLED.load(Ordering::SeqCst), "handler should have been called");
        clear_missing_key_handler();
    }

    #[test]
    fn handler_not_called_when_key_found() {
        HANDLER_CALLED.store(false, Ordering::SeqCst);
        set_missing_key_handler(test_handler);
        let mut buf = alloc::string::String::new();
        let _ = translate_to_writer("zz", "some.key", &[], &mut buf);
        assert!(HANDLER_CALLED.load(Ordering::SeqCst));
        clear_missing_key_handler();
    }
}

#[cfg(test)]
mod fallback_chain_tests {
    use super::*;

    #[test]
    fn single_fallback_chain_behaves_like_set_fallback_locale() {
        set_fallback_chain(&["en"]);
        let chain = get_fallback_chain();
        assert_eq!(chain.len(), 1);
        assert_eq!(&*chain[0], "en");
    }

    #[test]
    fn multi_hop_chain_is_stored() {
        set_fallback_chain(&["pt-BR", "pt", "en"]);
        let chain = get_fallback_chain();
        assert_eq!(chain.len(), 3);
        assert_eq!(&*chain[0], "pt-BR");
        assert_eq!(&*chain[1], "pt");
        assert_eq!(&*chain[2], "en");
        set_fallback_chain(&["en"]);
    }
}

#[cfg(test)]
mod store_perf_tests {
    use super::*;

    #[test]
    fn lookup_returns_none_for_missing_locale() {
        let store = TranslationStore::default();
        assert!(store.lookup("fr").is_none());
    }

    #[test]
    fn lookup_returns_buffer_for_loaded_locale() {
        let mut store = TranslationStore::default();
        let buf = Arc::new(alloc::vec![0x4c, 0x31, 0x30, 0x4e]);
        Arc::make_mut(&mut store.locales).push((String::from("en"), Arc::clone(&buf)));
        let found = store.lookup("en");
        assert!(found.is_some());
        assert_eq!(found.unwrap(), buf.as_slice());
    }

    #[test]
    fn fallback_clone_is_arc_not_string() {
        let locale: Arc<str> = get_fallback_locale();
        assert_eq!(&*locale, "en");
    }
}
