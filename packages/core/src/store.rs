extern crate alloc;
use crate::binary_format::BinaryFormatReader;
use crate::error::CoreResult;
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

/// Holds decompressed L10N binary data for a locale.
///
/// - `Owned` — heap-allocated, used by runtime-loaded `.pak` files.
///   `is_verified()` always returns `false` for this variant; runtime
///   ALWAYS verifies Owned data if a verify key is configured (see Prelude Rule 3).
/// - `Static` — compile-time embedded via `include_bytes!` or similar.
///   The `bool` is the `already_verified` flag passed at load time, stored
///   as-is and returned directly by `is_verified()`.
///
/// # no_std compatibility
///
/// - `StoreData::Static(&'static [u8], bool)` requires only `core` (no alloc).
/// - `StoreData::Owned(Arc<Vec<u8>>)` requires `alloc` (for `Arc` and `Vec`).
#[derive(Clone)]
pub enum StoreData {
    /// Runtime-loaded from a `.pak` file. Verification happens at runtime (if configured).
    Owned(Arc<Vec<u8>>),
    /// Compile-time embedded data. The `bool` is the `already_verified` flag
    /// passed via `load_static_bytes`. If `true`, build-time verification was performed.
    Static(&'static [u8], bool),
}

impl StoreData {
    /// Returns the underlying bytes regardless of variant.
    pub fn as_slice(&self) -> &[u8] {
        match self {
            StoreData::Owned(v) => v.as_slice(),
            StoreData::Static(s, _) => s,
        }
    }

    /// Returns `true` if this data has been cryptographically verified.
    ///
    /// - `Static` data returns the `already_verified` flag passed at load time
    ///   (build-time verification is assumed).
    /// - `Owned` data: returns `false`. Runtime verification depends on whether
    ///   `integrity::set_verify_key` was configured; this method does not check that.
    pub fn is_verified(&self) -> bool {
        match self {
            StoreData::Owned(_) => false,
            StoreData::Static(_, verified) => *verified,
        }
    }

    /// Returns `true` if this data is compile-time embedded (static).
    pub fn is_static(&self) -> bool {
        matches!(self, StoreData::Static(_, _))
    }
}

/// Manages loaded localization packages: maps locale codes to their decompressed binary buffers.
/// Uses a sorted `Vec` for O(log n) binary-search lookup and cache-friendly O(n) clone.
/// `locales` is `Arc`-wrapped so `clone()` on the whole store is O(1) when locales don't change.
pub struct TranslationStore {
    /// Sorted vector of locale-to-buffer mappings (Arc for O(1) clone). Binary-search for lookup.
    pub locales: Arc<Vec<(String, StoreData)>>,
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
    emit_locale_changed("*");
}

/// Returns `true` if `key` exists in `locale` or the configured fallback locale.
/// When `context` is `Some("male")`, it first tries `key_male` then `key`.
pub fn key_exists(locale: &str, key: &str, context: Option<&str>) -> bool {
    let chain = get_fallback_chain();
    read_store(|store| {
        let mut candidates = alloc::vec![locale];
        if let Some(p) = subtag_parent(locale) {
            candidates.push(p);
        }
        for fb in chain.iter() {
            candidates.push(fb.as_ref());
        }
        for loc in candidates {
            let check_key = |k: &str| -> bool {
                if let Some(buf) = store.lookup(loc) {
                    if let Ok(reader) = BinaryFormatReader::new(buf) {
                        if reader.lookup(k).is_some() { return true; }
                    }
                }
                false
            };
            if let Some(ctx) = context {
                if check_key(&alloc::format!("{}_{}", key, ctx)) { return true; }
            }
            if check_key(key) { return true; }
        }
        false
    })
}

/// Returns `true` if translations for `locale` are loaded.
pub fn locale_loaded(locale: &str) -> bool {
    read_store(|store| store.lookup(locale).is_some())
}

/// Returns the parent language tag by stripping the last subtag component.
/// `"en-US"` → `Some("en")`, `"zh-Hans-CN"` → `Some("zh-Hans")`, `"en"` → `None`.
fn subtag_parent(locale: &str) -> Option<&str> {
    let pos = locale.rfind(['-', '_'])?;
    if pos == 0 { return None; }
    Some(&locale[..pos])
}

type LocaleChangeFn = fn(locale: &str);
static LOCALE_CHANGE_CALLBACKS: core::sync::atomic::AtomicPtr<()> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
static LOCALE_CHANGE_BOXED: core::sync::atomic::AtomicPtr<()> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// Registers a callback invoked when a locale is loaded or cleared.
pub fn on_locale_changed(callback: LocaleChangeFn) {
    LOCALE_CHANGE_CALLBACKS.store(callback as *mut (), core::sync::atomic::Ordering::Release);
}

/// Registers a boxed dynamic callback for WASM bindings.
pub fn on_locale_changed_boxed(callback: alloc::boxed::Box<dyn Fn(&str) + Send>) {
    let ptr = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(callback));
    LOCALE_CHANGE_BOXED.store(ptr as *mut (), core::sync::atomic::Ordering::Release);
}

/// Removes all locale change callbacks.
pub fn clear_locale_changed_callbacks() {
    LOCALE_CHANGE_CALLBACKS.store(core::ptr::null_mut(), core::sync::atomic::Ordering::Release);
    let old = LOCALE_CHANGE_BOXED.swap(core::ptr::null_mut(), core::sync::atomic::Ordering::Release);
    if !old.is_null() {
        unsafe { drop(alloc::boxed::Box::from_raw(old as *mut Box<dyn Fn(&str) + Send>)); }
    }
}

pub(crate) fn emit_locale_changed(locale: &str) {
    let ptr = LOCALE_CHANGE_CALLBACKS.load(core::sync::atomic::Ordering::Acquire);
    if !ptr.is_null() {
        let f: LocaleChangeFn = unsafe { core::mem::transmute(ptr) };
        f(locale);
    }
    let boxed_ptr = LOCALE_CHANGE_BOXED.load(core::sync::atomic::Ordering::Acquire);
    if !boxed_ptr.is_null() {
        unsafe { (*(boxed_ptr as *mut Box<dyn Fn(&str) + Send>))(locale); }
    }
}

/// Attempts to translate `key` from `locale` in `store`, writing to `writer`.
/// Returns `true` if translation succeeded.
#[inline]
fn try_locale<W: core::fmt::Write>(
    store: &TranslationStore,
    locale: &str,
    key: &str,
    context: Option<&str>,
    params: &[(&str, &str)],
    writer: &mut W,
) -> bool {
    // Try context suffix first: key_male → key
    if let Some(ctx) = context {
        let ctx_key = alloc::format!("{}_{}", key, ctx);
        if let Some(buf) = store.lookup(locale) {
            if let Ok(reader) = BinaryFormatReader::new(buf) {
                if let Some(bytecode) = reader.lookup(&ctx_key) {
                    if format_message(bytecode, locale, params, writer).is_ok() {
                        return true;
                    }
                    crate::metrics::inc_format_errors();
                }
            }
        }
    }
    if let Some(buf) = store.lookup(locale) {
        if let Ok(reader) = BinaryFormatReader::new(buf) {
            if let Some(bytecode) = reader.lookup(key) {
                if format_message(bytecode, locale, params, writer).is_ok() {
                    return true;
                }
                crate::metrics::inc_format_errors();
            }
        }
    }
    false
}

/// Helper function to translate a key directly into a caller-provided Writer.
/// When `context` is `Some("male")`, it first tries `key_male` then falls back to `key`.
pub fn translate_to_writer<W: core::fmt::Write>(
    locale: &str,
    key: &str,
    context: Option<&str>,
    params: &[(&str, &str)],
    writer: &mut W,
) -> CoreResult<()> {
    crate::metrics::inc_total_translations();

    #[cfg(feature = "std")]
    {
        log::debug!("translate: locale={}, key={}", locale, key);
    }

    let chain = get_fallback_chain();

    let success = read_store(|store| {
        // 1. Try exact locale match
        if try_locale(store, locale, key, context, params, writer) {
            return Some(());
        }

        // 2. BCP-47 subtag negotiation: en-US → en
        if let Some(parent) = subtag_parent(locale) {
            if parent != locale && try_locale(store, parent, key, context, params, writer) {
                return Some(());
            }
        }

        // 3. Walk the configured fallback chain
        for fb in chain.iter() {
            let fb_str: &str = fb.as_ref();
            if fb_str == locale { continue; }
            if let Some(parent) = subtag_parent(locale) {
                if fb_str == parent { continue; }
            }
            if try_locale(store, fb_str, key, context, params, writer) {
                return Some(());
            }
        }
        None
    });

    if success.is_some() {
        crate::metrics::inc_cache_hits();
        Ok(())
    } else {
        crate::metrics::inc_cache_misses();
        call_missing_key_handler(locale, key);
        writer
            .write_str(key)
            .map_err(|_| crate::CoreError::IoError("Failed to write key fallback"))?;
        Ok(())
    }
}

/// Translates a key for a given locale, dynamically interpolating parameters,
/// and returning an allocated String.
pub fn translate(locale: &str, key: &str, context: Option<&str>, params: &[(&str, &str)]) -> String {
    let mut buf = String::new();
    let _ = translate_to_writer(locale, key, context, params, &mut buf);
    buf
}

#[cfg(test)]
mod store_data_tests {
    use super::*;

    #[test]
    fn store_data_owned_as_slice() {
        let data = StoreData::Owned(Arc::new(vec![0x01, 0x02]));
        assert_eq!(data.as_slice(), &[0x01, 0x02]);
        assert!(!data.is_verified());
        assert!(!data.is_static());
    }

    #[test]
    fn store_data_static_verified() {
        static BYTES: &[u8] = &[0x03, 0x04];
        let data = StoreData::Static(BYTES, true);
        assert_eq!(data.as_slice(), &[0x03, 0x04]);
        assert!(data.is_verified());
        assert!(data.is_static());
    }

    #[test]
    fn store_data_static_unverified() {
        static BYTES: &[u8] = &[0x05];
        let data = StoreData::Static(BYTES, false);
        assert!(!data.is_verified());
        assert!(data.is_static());
    }

    #[test]
    fn store_data_clone() {
        let data = StoreData::Owned(Arc::new(vec![42]));
        let cloned = data.clone();
        assert_eq!(data.as_slice(), cloned.as_slice());
    }
}

#[cfg(all(test, feature = "std"))]
mod missing_key_tests {
    use super::*;
    use core::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    static MISSING_KEY_MUTEX: Mutex<()> = Mutex::new(());
    static HANDLER_CALLED: AtomicBool = AtomicBool::new(false);

    fn test_handler(locale: &str, key: &str) {
        HANDLER_CALLED.store(true, Ordering::SeqCst);
        let _ = (locale, key);
    }

    #[test]
    fn handler_called_on_missing_key() {
        let _lock = MISSING_KEY_MUTEX.lock().unwrap();
        clear_missing_key_handler();
        HANDLER_CALLED.store(false, Ordering::SeqCst);
        set_missing_key_handler(test_handler);
        let mut buf = alloc::string::String::new();
        let _ = translate_to_writer("xx", "nonexistent.key", None, &[], &mut buf);
        assert!(HANDLER_CALLED.load(Ordering::SeqCst), "handler should have been called");
        clear_missing_key_handler();
    }

    #[test]
    fn handler_called_when_key_not_found() {
        let _lock = MISSING_KEY_MUTEX.lock().unwrap();
        clear_missing_key_handler();
        HANDLER_CALLED.store(false, Ordering::SeqCst);
        set_missing_key_handler(test_handler);
        let mut buf = alloc::string::String::new();
        let _ = translate_to_writer("zz", "nonexistent", None, &[], &mut buf);
        assert!(HANDLER_CALLED.load(Ordering::SeqCst), "handler should be called for missing key");
        clear_missing_key_handler();
    }
}

#[cfg(all(test, feature = "std"))]
mod fallback_chain_tests {
    use super::*;
    use std::sync::Mutex;
    static FB_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn single_fallback_chain_behaves_like_set_fallback_locale() {
        let _lock = FB_MUTEX.lock().unwrap();
        set_fallback_chain(&["en"]);
        let chain = get_fallback_chain();
        assert_eq!(chain.len(), 1);
        assert_eq!(&*chain[0], "en");
    }

    #[test]
    fn multi_hop_chain_is_stored() {
        let _lock = FB_MUTEX.lock().unwrap();
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
mod bcp47_tests {
    use super::*;

    #[test]
    fn subtag_parent_strips_last_component() {
        assert_eq!(subtag_parent("en-US"),    Some("en"));
        assert_eq!(subtag_parent("zh-Hans-CN"), Some("zh-Hans"));
        assert_eq!(subtag_parent("pt_BR"),    Some("pt"));
    }

    #[test]
    fn subtag_parent_returns_none_for_root_tag() {
        assert_eq!(subtag_parent("en"),  None);
        assert_eq!(subtag_parent("fr"),  None);
        assert_eq!(subtag_parent(""),    None);
    }

    #[test]
    fn subtag_parent_handles_underscore_separator() {
        assert_eq!(subtag_parent("zh_Hant"), Some("zh"));
    }

    #[test]
    fn subtag_parent_dash_at_start_returns_none() {
        // If the separator is at position 0, should return None
        assert_eq!(subtag_parent("-en"), None);
    }
}

#[cfg(test)]
mod locale_change_callback_tests {
    use super::*;
    use core::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn locale_changed_callback_invoked() {
        static CALLED: AtomicBool = AtomicBool::new(false);
        on_locale_changed(|_locale| {
            CALLED.store(true, Ordering::SeqCst);
        });
        emit_locale_changed("es");
        assert!(CALLED.load(Ordering::SeqCst));
        clear_locale_changed_callbacks();
    }

    #[test]
    fn boxed_callback_invoked() {
        static CALLED: AtomicBool = AtomicBool::new(false);
        on_locale_changed_boxed(alloc::boxed::Box::new(|_locale| {
            CALLED.store(true, Ordering::SeqCst);
        }));
        emit_locale_changed("de");
        assert!(CALLED.load(Ordering::SeqCst));
        clear_locale_changed_callbacks();
    }

    #[test]
    fn cleared_callbacks_not_invoked() {
        clear_locale_changed_callbacks();
        static CALLED: AtomicBool = AtomicBool::new(false);
        on_locale_changed(|_locale| {
            CALLED.store(true, Ordering::SeqCst);
        });
        clear_locale_changed_callbacks();
        emit_locale_changed("fr");
        assert!(!CALLED.load(Ordering::SeqCst));
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
        Arc::make_mut(&mut store.locales).push((String::from("en"), StoreData::Owned(Arc::clone(&buf))));
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

#[cfg(test)]
mod store_extra_tests {
    use super::*;

    fn make_binary_with_key(key: &str, val: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&1u32.to_be_bytes());
        let index_offset: u32 = 16;
        buf.extend_from_slice(&index_offset.to_be_bytes());
        let index_count: u32 = 1;
        buf.extend_from_slice(&index_count.to_be_bytes());
        let key_off = 16 + 16;
        let val_off = key_off + key.len() as u32;
        buf.extend_from_slice(&key_off.to_be_bytes());
        buf.extend_from_slice(&(key.len() as u32).to_be_bytes());
        buf.extend_from_slice(&val_off.to_be_bytes());
        buf.extend_from_slice(&(val.len() as u32).to_be_bytes());
        buf.extend_from_slice(key.as_bytes());
        buf.extend_from_slice(val);
        buf
    }

    fn load_locale_with_key(locale: &str, key: &str, val: &[u8]) {
        let locales = Arc::new(alloc::vec![(
            String::from(locale),
            StoreData::Owned(Arc::new(make_binary_with_key(key, val))),
        )]);
        let chain = get_fallback_chain();
        swap_store(TranslationStore {
            locales,
            fallback_chain: chain,
        });
    }

    use core::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;
    static EXTRA_MUTEX: Mutex<()> = Mutex::new(());
    fn lock_extra() -> std::sync::MutexGuard<'static, ()> {
        EXTRA_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn clear_translations_preserves_fallback() {
        let _lock = lock_extra();
        clear_translations();
        let locale = get_fallback_locale();
        assert_eq!(&*locale, "en");
    }

    #[test]
    fn locale_loaded_returns_false_for_empty() {
        let _lock = lock_extra();
        clear_translations();
        assert!(!locale_loaded("en"));
    }

    #[test]
    fn key_exists_returns_false_without_data() {
        let _lock = lock_extra();
        clear_translations();
        assert!(!key_exists("en", "some.key", None));
    }

    #[test]
    fn get_fallback_chain_default() {
        let _lock = lock_extra();
        let chain = get_fallback_chain();
        assert!(!chain.is_empty());
    }

    #[test]
    fn set_fallback_chain_empty() {
        let _lock = lock_extra();
        set_fallback_chain(&[]);
        let chain = get_fallback_chain();
        assert_eq!(chain.len(), 0);
        set_fallback_chain(&["en"]);
    }

    #[test]
    fn on_locale_changed_and_clear() {
        on_locale_changed(|_| {});
        clear_locale_changed_callbacks();
    }

    #[test]
    fn try_reclaim_does_not_panic() {
        try_reclaim();
    }

    #[test]
    fn translate_key_not_found_returns_key() {
        let _lock = lock_extra();
        clear_translations();
        let result = translate("xx", "missing.key", None, &[]);
        assert_eq!(result, "missing.key");
    }

    #[test]
    fn clear_missing_key_handler_is_safe() {
        clear_missing_key_handler();
    }

    #[test]
    fn set_fallback_locale_and_verify() {
        let _lock = lock_extra();
        set_fallback_locale("de");
        let locale = get_fallback_locale();
        assert_eq!(&*locale, "de");
        set_fallback_locale("en");
    }

    #[test]
    fn key_exists_success_hits_lookup_ok_path() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key("en", "greeting", &[0x01, 0x00, 0x00, 0x00, 0x05, b'H', b'e', b'l', b'l', b'o']);
        assert!(key_exists("en", "greeting", None), "should find existing key");
    }

    #[test]
    fn key_exists_subtag_parent_pushes_candidate() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key("en", "greeting", &[0x01, 0x00, 0x00, 0x00, 0x05, b'H', b'e', b'l', b'l', b'o']);
        assert!(
            key_exists("en-US", "greeting", None),
            "should find key via subtag parent 'en'"
        );
    }

    #[test]
    fn key_exists_with_context_suffix_hit() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key("en", "greeting_male", &[0x01, 0x00, 0x00, 0x00, 0x05, b'H', b'e', b'l', b'l', b'o']);
        assert!(
            key_exists("en", "greeting", Some("male")),
            "should find context-suffixed key"
        );
    }

    #[test]
    fn translate_subtag_parent_success() {
        let _lock = lock_extra();
        clear_translations();
        let val: Vec<u8> = vec![0x01, 0x00, 0x00, 0x00, 0x0B, b'H', b'e', b'l', b'l', b'o', b' ', b'W', b'o', b'r', b'l', b'd'];
        load_locale_with_key("en", "greeting", &val);
        let result = translate("en-US", "greeting", None, &[]);
        assert_eq!(result, "Hello World", "should resolve via parent en");
    }

    #[test]
    fn translate_fallback_chain_skips_parent() {
        let _lock = lock_extra();
        clear_translations();
        // Load "fr" with greeting, set fallback to ["fr"], request "en-US"
        // Since "en-US"|"en" has no data, and fallback "fr" != parent "en", "fr" IS checked
        let val: Vec<u8> = vec![0x01, 0x00, 0x00, 0x00, 0x0B, b'H', b'e', b'l', b'l', b'o', b' ', b'W', b'o', b'r', b'l', b'd'];
        load_locale_with_key("fr", "greeting", &val);
        set_fallback_chain(&["fr"]);
        let result = translate("en-US", "greeting", None, &[]);
        assert_eq!(result, "Hello World", "should resolve via fallback fr since fr != parent en");
        set_fallback_chain(&["en"]);
    }

    #[test]
    fn try_locale_inc_format_errors_on_bad_bytecode() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key("en", "broken", &[0xFF]);
        let before = crate::metrics::format_errors();
        let result = translate("en", "broken", None, &[]);
        let after = crate::metrics::format_errors();
        assert_eq!(result, "broken", "bad bytecode falls through to key-as-text");
        assert!(after > before, "format_errors should increase on bad bytecode");
    }

    #[test]
    fn try_locale_context_format_error_increments_counter() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key("en", "broken_male", &[0xFF]);
        let before = crate::metrics::format_errors();
        let result = translate("en", "broken", Some("male"), &[]);
        let after = crate::metrics::format_errors();
        assert_eq!(result, "broken", "bad context bytecode falls through");
        assert!(after >= before, "format_errors should increase for bad context bytecode");
    }
}
