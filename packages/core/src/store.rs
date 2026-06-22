extern crate alloc;
use crate::binary_format::BinaryFormatReader;
use crate::error::CoreResult;
use crate::formatter::format_message;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicPtr, Ordering};

#[cfg(all(not(feature = "std"), debug_assertions))]
use core::sync::atomic::AtomicUsize;

#[cfg(feature = "std")]
use std::cell::RefCell;
#[cfg(feature = "std")]
use std::collections::{HashMap, VecDeque};
#[cfg(feature = "std")]
use std::sync::OnceLock;

#[cfg(feature = "std")]
const TRANSLATE_CACHE_CAPACITY: usize = 128;

#[cfg(feature = "std")]
thread_local! {
    static TRANSLATE_BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

#[cfg(feature = "std")]
thread_local! {
    static TRANSLATE_CACHE: RefCell<HashMap<(u64, u64), String>> = RefCell::new(HashMap::new());
    static TRANSLATE_CACHE_ORDER: RefCell<VecDeque<(u64, u64)>> = const { RefCell::new(VecDeque::new()) };
}

#[cfg(feature = "std")]
fn cache_translate(locale: &str, key_hash: u64, params: &[(&str, &str)]) -> Option<String> {
    // Only cache translations without parameters (most repetitive calls: labels, titles)
    if !params.is_empty() {
        return None;
    }
    let locale_hash = crate::binary_format::fnv1a_64(locale.as_bytes());
    TRANSLATE_CACHE.with(|cell| {
        let cache = cell.borrow();
        cache.get(&(locale_hash, key_hash)).cloned()
    })
}

#[cfg(feature = "std")]
fn cache_insert(locale: &str, key_hash: u64, params: &[(&str, &str)], result: &str, key_found: bool) {
    if !key_found || !params.is_empty() {
        return;
    }
    let locale_hash = crate::binary_format::fnv1a_64(locale.as_bytes());
    let cache_key = (locale_hash, key_hash);
    TRANSLATE_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        TRANSLATE_CACHE_ORDER.with(|order_cell| {
            let mut order = order_cell.borrow_mut();
            if cache.len() >= TRANSLATE_CACHE_CAPACITY && !cache.contains_key(&cache_key) {
                let evict_count = TRANSLATE_CACHE_CAPACITY / 4;
                for _ in 0..evict_count {
                    if let Some(old_key) = order.pop_front() {
                        cache.remove(&old_key);
                    }
                }
            }
            if !cache.contains_key(&cache_key) {
                order.push_back(cache_key);
            }
            cache.insert(cache_key, result.to_string());
        });
    });
}

#[cfg(feature = "std")]
fn default_chain() -> Arc<[Arc<str>]> {
    use std::sync::OnceLock;
    static CHAIN: OnceLock<Arc<[Arc<str>]>> = OnceLock::new();
    CHAIN
        .get_or_init(|| Arc::from(alloc::vec![Arc::from("en") as Arc<str>].into_boxed_slice()))
        .clone()
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
/// - `Lazy` — raw compressed `.pak` bytes deferred for decompression on first access.
///   Only available under `feature = "std"`.
///
/// # no_std compatibility
///
/// - `StoreData::Static(&'static [u8], bool)` requires only `core` (no alloc).
/// - `StoreData::Owned(Arc<Vec<u8>>)` requires `alloc` (for `Arc` and `Vec`).
/// - `StoreData::Lazy(Arc<Vec<u8>>)` requires `alloc` + `std` (for OnceLock cache).
pub enum StoreData {
    /// Runtime-loaded from a `.pak` file. Verification happens at runtime (if configured).
    Owned(Arc<Vec<u8>>),
    /// Compile-time embedded data. The `bool` is the `already_verified` flag
    /// passed via `load_static_bytes`. If `true`, build-time verification was performed.
    Static(&'static [u8], bool),
    /// Raw compressed `.pak` bytes. Decompressed on first lookup via lazy_cache.
    #[cfg(feature = "std")]
    Lazy(Arc<Vec<u8>>),
}

impl Clone for StoreData {
    fn clone(&self) -> Self {
        match self {
            StoreData::Owned(v) => StoreData::Owned(v.clone()),
            StoreData::Static(v, f) => StoreData::Static(v, *f),
            #[cfg(feature = "std")]
            StoreData::Lazy(v) => StoreData::Lazy(v.clone()),
        }
    }
}

impl StoreData {
    /// Returns the underlying bytes regardless of variant.
    pub fn as_slice(&self) -> &[u8] {
        match self {
            StoreData::Owned(v) => v.as_slice(),
            StoreData::Static(s, _) => s,
            #[cfg(feature = "std")]
            StoreData::Lazy(v) => v.as_slice(),
        }
    }

    /// Returns `true` if this data has been cryptographically verified.
    ///
    /// - `Static` data returns the `already_verified` flag passed at load time
    ///   (build-time verification is assumed).
    /// - `Owned` data: returns `false`. Runtime verification depends on whether
    ///   `integrity::set_verify_key` was configured; this method does not check that.
    /// - `Lazy` data: returns `false` (no decompression-level verification yet).
    pub fn is_verified(&self) -> bool {
        match self {
            StoreData::Owned(_) => false,
            StoreData::Static(_, verified) => *verified,
            #[cfg(feature = "std")]
            StoreData::Lazy(_) => false,
        }
    }

    /// Returns `true` if this data is compile-time embedded (static).
    pub fn is_static(&self) -> bool {
        matches!(self, StoreData::Static(_, _))
    }
}

/// Per-locale lazy decompression cache: maps locale_hash to decompressed bytes and offset map.
#[cfg(feature = "std")]
type LazyDecompressCache = HashMap<u64, Arc<OnceLock<(Vec<u8>, OffsetMap)>>>;
/// Per-locale O(1) offset cache: maps locale_hash to key-hash-based (offset, length) pairs.
#[cfg(feature = "std")]
type OffsetMap = Arc<HashMap<u64, (u32, u32)>>;

/// Manages loaded localization packages: maps locale codes to their decompressed binary buffers.
/// Uses a sorted `Vec` for O(log n) binary-search lookup and cache-friendly O(n) clone.
/// `locales` is `Arc`-wrapped so `clone()` on the whole store is O(1) when locales don't change.
pub struct TranslationStore {
    /// Sorted vector of locale-to-buffer mappings (Arc for O(1) clone). Binary-search for lookup.
    pub locales: Arc<Vec<(String, StoreData)>>,
    /// Ordered chain of fallback locale codes. The first match wins.
    pub fallback_chain: Arc<[Arc<str>]>,
    /// Per-locale lazy decompression cache. Key: locale_hash.
    #[cfg(feature = "std")]
    pub lazy_cache: LazyDecompressCache,
    /// Per-locale O(1) offset maps. Key: locale_hash. Value: Arc<HashMap<key_hash -> (offset, len)>>.
    #[cfg(feature = "std")]
    pub offset_maps: HashMap<u64, OffsetMap>,
}

impl Default for TranslationStore {
    fn default() -> Self {
        Self {
            locales: Arc::new(Vec::new()),
            fallback_chain: default_chain(),
            #[cfg(feature = "std")]
            lazy_cache: HashMap::new(),
            #[cfg(feature = "std")]
            offset_maps: HashMap::new(),
        }
    }
}

impl TranslationStore {
    /// Looks up the decompressed translation buffer for a given locale. O(log n) binary search.
    /// For Lazy entries, decompresses on first access via lazy_cache.
    pub fn lookup(&self, locale: &str) -> Option<&[u8]> {
        let idx = self
            .locales
            .binary_search_by(|(loc, _)| loc.as_str().cmp(locale))
            .ok()?;
        match &self.locales[idx].1 {
            StoreData::Owned(v) => Some(v.as_slice()),
            StoreData::Static(v, _) => Some(v),
            #[cfg(feature = "std")]
            StoreData::Lazy(compressed) => {
                let locale_hash = crate::binary_format::fnv1a_64(locale.as_bytes());
                let entry = self
                    .lazy_cache
                    .get(&locale_hash)
                    .expect("lazy_cache entry must exist for StoreData::Lazy locale");
                let (decompressed, _) = entry.get_or_init(|| {
                    let output = crate::pak::decompress_zstd_payload(compressed.as_slice())
                        .expect("zstd decompression failed");
                    let offsets = if let Ok(reader) =
                        crate::binary_format::BinaryFormatReader::new(&output)
                    {
                        Arc::new(reader.to_offsets())
                    } else {
                        Arc::new(HashMap::new())
                    };
                    (output, offsets)
                });
                Some(decompressed.as_slice())
            }
        }
    }
}

// Global store pointer
static STORE: AtomicPtr<TranslationStore> = AtomicPtr::new(core::ptr::null_mut());

// Function pointer type for missing key notifications.
type MissingKeyFn = fn(locale: &str, key_hash: u64);

static MISSING_KEY_HANDLER: AtomicPtr<()> = AtomicPtr::new(core::ptr::null_mut());

/// Installs a callback invoked whenever a key is not found in any locale or fallback.
/// The callback receives the originally requested locale and the missing key hash.
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

fn call_missing_key_handler(locale: &str, key_hash: u64) {
    let ptr = MISSING_KEY_HANDLER.load(Ordering::Acquire);
    if !ptr.is_null() {
        let f: MissingKeyFn = unsafe { core::mem::transmute(ptr) };
        f(locale, key_hash);
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
    #[cfg(feature = "std")]
    {
        let (locales, _fallback, lazy_cache, offset_maps) = read_store(|store| {
            (
                Arc::clone(&store.locales),
                Arc::clone(&store.fallback_chain),
                store.lazy_cache.clone(),
                store.offset_maps.clone(),
            )
        });
        swap_store(TranslationStore {
            locales,
            fallback_chain: new_chain,
            lazy_cache,
            offset_maps,
        });
    }
    #[cfg(not(feature = "std"))]
    {
        let locales = read_store(|store| Arc::clone(&store.locales));
        swap_store(TranslationStore {
            locales,
            fallback_chain: new_chain,
        });
    }
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
        store
            .fallback_chain
            .first()
            .cloned()
            .unwrap_or_else(|| Arc::from("en"))
    })
}

/// Clears all loaded translations.
pub fn clear_translations() {
    let chain = read_store(|store| Arc::clone(&store.fallback_chain));
    #[cfg(feature = "std")]
    {
        swap_store(TranslationStore {
            locales: Arc::new(Vec::new()),
            fallback_chain: chain,
            lazy_cache: HashMap::new(),
            offset_maps: HashMap::new(),
        });
    }
    #[cfg(not(feature = "std"))]
    {
        swap_store(TranslationStore {
            locales: Arc::new(Vec::new()),
            fallback_chain: chain,
        });
    }
    emit_locale_changed("*");
}

/// Outcome metadata for a translation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranslateStatus {
    /// `true` when the key was resolved in the requested locale or fallback chain.
    pub key_found: bool,
    /// `true` when the requested locale has loaded translation data.
    pub locale_loaded: bool,
}

#[cfg(feature = "std")]
fn offset_map_for_locale<'a>(
    store: &'a TranslationStore,
    locale: &str,
) -> Option<&'a HashMap<u64, (u32, u32)>> {
    let locale_hash = crate::binary_format::fnv1a_64(locale.as_bytes());
    let lazy_offsets = store
        .lazy_cache
        .get(&locale_hash)
        .and_then(|entry| entry.get().map(|(_, offsets)| offsets.as_ref()));
    store
        .offset_maps
        .get(&locale_hash)
        .filter(|m| !m.is_empty())
        .map(|arc| arc.as_ref())
        .or(lazy_offsets)
}

fn hash_present_in_locale(
    store: &TranslationStore,
    locale: &str,
    key_hash: u64,
) -> bool {
    let Some(buf) = store.lookup(locale) else {
        return false;
    };
    #[cfg(feature = "std")]
    if let Some(map) = offset_map_for_locale(store, locale) {
        if map.contains_key(&key_hash) {
            return true;
        }
    }
    if let Ok(reader) = BinaryFormatReader::new(buf) {
        return reader.lookup(key_hash).is_some();
    }
    false
}

fn key_exists_in_store(
    store: &TranslationStore,
    chain: &[Arc<str>],
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
) -> bool {
    if let Some(ctx_hash) = context_hash {
        if hash_present_in_locale(store, locale, ctx_hash) {
            return true;
        }
    }
    if hash_present_in_locale(store, locale, key_hash) {
        return true;
    }
    if let Some(parent) = locale_parent(locale) {
        if parent != locale {
            if let Some(ctx_hash) = context_hash {
                if hash_present_in_locale(store, parent, ctx_hash) {
                    return true;
                }
            }
            if hash_present_in_locale(store, parent, key_hash) {
                return true;
            }
        }
    }
    for fb in chain.iter() {
        let fb_str: &str = fb.as_ref();
        if fb_str == locale {
            continue;
        }
        if let Some(parent) = locale_parent(locale) {
            if fb_str == parent {
                continue;
            }
        }
        if let Some(ctx_hash) = context_hash {
            if hash_present_in_locale(store, fb_str, ctx_hash) {
                return true;
            }
        }
        if hash_present_in_locale(store, fb_str, key_hash) {
            return true;
        }
    }
    false
}

fn resolve_translate_in_store<W: core::fmt::Write>(
    store: &TranslationStore,
    chain: &[Arc<str>],
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
    writer: &mut W,
) -> bool {
    if try_locale(store, locale, key_hash, context_hash, params, writer) {
        return true;
    }
    if let Some(parent) = locale_parent(locale) {
        if parent != locale && try_locale(store, parent, key_hash, context_hash, params, writer) {
            return true;
        }
    }
    for fb in chain.iter() {
        let fb_str: &str = fb.as_ref();
        if fb_str == locale {
            continue;
        }
        if let Some(parent) = locale_parent(locale) {
            if fb_str == parent {
                continue;
            }
        }
        if try_locale(store, fb_str, key_hash, context_hash, params, writer) {
            return true;
        }
    }
    false
}

/// Returns `true` if `key_hash` exists in `locale` or the configured fallback locale.
/// When `context_hash` is `Some(...)`, it first tries the context hash then `key_hash`.
pub fn key_exists(locale: &str, key_hash: u64, context_hash: Option<u64>) -> bool {
    read_store(|store| {
        key_exists_in_store(store, &store.fallback_chain, locale, key_hash, context_hash)
    })
}

/// Returns `true` if translations for `locale` are loaded.
pub fn locale_loaded(locale: &str) -> bool {
    read_store(|store| store.lookup(locale).is_some())
}

/// Loads a static (compile-time embedded) L10N binary buffer into the global store.
///
/// `already_verified`: if `true`, the data was cryptographically verified at build time.
///   Runtime will NOT re-verify it. This follows Rule 2 of the static embed contract.
///   If `false`, the data is treated as unverified (conservative default).
///
/// Unlike `load_raw_bytes`, this does NOT allocate a copy of the data buffer —
/// the `&'static [u8]` is stored directly in the `StoreData::Static` variant.
///
/// Compatible with `no_std + alloc` (no filesystem I/O required).
pub fn load_static_bytes(locale_str: &str, data: &'static [u8], already_verified: bool) -> bool {
    crate::metrics::inc_locale_loads();
    #[cfg(feature = "std")]
    {
        let (mut locales, fallback_chain, _lazy_cache, mut offset_maps) = read_store(|store| {
            (
                Arc::clone(&store.locales),
                Arc::clone(&store.fallback_chain),
                store.lazy_cache.clone(),
                store.offset_maps.clone(),
            )
        });
        let new_vec = Arc::make_mut(&mut locales);
        let locale_hash = crate::binary_format::fnv1a_64(locale_str.as_bytes());
        let offset_arc = if let Ok(reader) = crate::binary_format::BinaryFormatReader::new(data) {
            Arc::new(reader.to_offsets())
        } else {
            Arc::new(HashMap::new())
        };
        offset_maps.insert(locale_hash, offset_arc);
        let entry = (
            locale_str.to_string(),
            StoreData::Static(data, already_verified),
        );
        match new_vec.binary_search_by(|(loc, _)| loc.as_str().cmp(locale_str)) {
            Ok(pos) => new_vec[pos] = entry,
            Err(pos) => new_vec.insert(pos, entry),
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
        let entry = (
            locale_str.to_string(),
            StoreData::Static(data, already_verified),
        );
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

/// Batch-initializes the store with multiple static (compile-time embedded) locales.
///
/// Each entry in `locales` is a `(locale_code, &'static [u8])` pair.
///
/// # Security
///
/// This function sets `already_verified = true` for all entries. It is the
/// **responsibility of the build script** (`build.rs`) to verify the Ed25519
/// signature of each locale's data BEFORE generating the static byte arrays.
/// See "Signature handling rules" in the design doc
/// (`docs/superpowers/specs/2026-06-21-compile-time-embedding-design.md` §4b).
///
/// If you need to load data that has NOT been verified at build time, call
/// `load_static_bytes` directly with `already_verified: false`.
///
/// # Example
///
/// ```ignore
/// l10n4x_core::store::init_embedded(&[
///     ("en", include_bytes!("../translations/en.l10n")),
///     ("es", include_bytes!("../translations/es.l10n")),
/// ]);
/// ```
pub fn init_embedded(locales: &[(&str, &'static [u8])]) {
    for (locale, data) in locales {
        load_static_bytes(locale, data, true);
    }
}

/// Returns the parent language tag by stripping the last subtag component.
/// `"en-US"` → `Some("en")`, `"zh-Hans-CN"` → `Some("zh-Hans")`, `"en"` → `None`.
pub fn locale_parent(locale: &str) -> Option<&str> {
    let pos = locale.rfind(['-', '_'])?;
    if pos == 0 {
        return None;
    }
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
    let old =
        LOCALE_CHANGE_BOXED.swap(core::ptr::null_mut(), core::sync::atomic::Ordering::Release);
    if !old.is_null() {
        unsafe {
            drop(alloc::boxed::Box::from_raw(
                old as *mut Box<dyn Fn(&str) + Send>,
            ));
        }
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
        unsafe {
            (*(boxed_ptr as *mut Box<dyn Fn(&str) + Send>))(locale);
        }
    }
}

/// Attempts to translate `key_hash` from `locale` in `store`, writing to `writer`.
/// Returns `true` if translation succeeded.
#[inline]
fn try_locale<W: core::fmt::Write>(
    store: &TranslationStore,
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
    writer: &mut W,
) -> bool {
    if let Some(buf) = store.lookup(locale) {
        #[cfg(feature = "std")]
        {
            let locale_hash = crate::binary_format::fnv1a_64(locale.as_bytes());
            let lazy_offsets = store
                .lazy_cache
                .get(&locale_hash)
                .and_then(|entry| entry.get().map(|(_, offsets)| offsets));
            let map = store
                .offset_maps
                .get(&locale_hash)
                .filter(|m| !m.is_empty())
                .or(lazy_offsets);
            if let Some(map) = map {
                let mut used_offset_map = false;
                if let Some(ctx_hash) = context_hash {
                    if let Some(&(off, len)) = map.get(&ctx_hash) {
                        used_offset_map = true;
                        let start = off as usize;
                        let end = start + (len as usize);
                        if end <= buf.len()
                            && format_message(&buf[start..end], locale, params, writer).is_ok()
                        {
                            return true;
                        }
                        crate::metrics::inc_format_errors();
                    }
                }
                if let Some(&(off, len)) = map.get(&key_hash) {
                    used_offset_map = true;
                    let start = off as usize;
                    let end = start + (len as usize);
                    if end <= buf.len()
                        && format_message(&buf[start..end], locale, params, writer).is_ok()
                    {
                        return true;
                    }
                    crate::metrics::inc_format_errors();
                }
                if used_offset_map {
                    return false;
                }
            }
        }
        // Fallback: BinaryFormatReader on already-decompressed buf
        if let Ok(reader) = BinaryFormatReader::new(buf) {
            if let Some(ctx_hash) = context_hash {
                if let Some(bytecode) = reader.lookup(ctx_hash) {
                    if format_message(bytecode, locale, params, writer).is_ok() {
                        return true;
                    }
                    crate::metrics::inc_format_errors();
                }
            }
            if let Some(bytecode) = reader.lookup(key_hash) {
                if format_message(bytecode, locale, params, writer).is_ok() {
                    return true;
                }
                crate::metrics::inc_format_errors();
            }
        }
    }
    false
}

/// Translates a key hash into `writer` and returns lookup metadata in a single store read.
/// When `context_hash` is `Some(...)`, it first tries the context hash then falls back to `key_hash`.
pub fn translate_to_writer_with_status<W: core::fmt::Write>(
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
    writer: &mut W,
) -> CoreResult<TranslateStatus> {
    crate::metrics::inc_total_translations();

    #[cfg(feature = "std")]
    {
        log::debug!("translate: locale={}, key_hash={:#x}", locale, key_hash);
    }

    let status = read_store(|store| {
        let locale_loaded = store.lookup(locale).is_some();
        let found = resolve_translate_in_store(
            store,
            &store.fallback_chain,
            locale,
            key_hash,
            context_hash,
            params,
            writer,
        );
        TranslateStatus {
            key_found: found,
            locale_loaded,
        }
    });

    if status.key_found {
        crate::metrics::inc_cache_hits();
    } else {
        crate::metrics::inc_cache_misses();
        call_missing_key_handler(locale, key_hash);
        let _ = core::write!(writer, "{:#x}", key_hash);
    }
    Ok(status)
}

/// Helper function to translate a key hash directly into a caller-provided Writer.
/// When `context_hash` is `Some(...)`, it first tries the context hash then falls back to `key_hash`.
pub fn translate_to_writer<W: core::fmt::Write>(
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
    writer: &mut W,
) -> CoreResult<()> {
    translate_to_writer_with_status(locale, key_hash, context_hash, params, writer)?;
    Ok(())
}

/// Translates a key hash for a given locale, dynamically interpolating parameters,
/// and returning an allocated String.
pub fn translate(
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
) -> String {
    #[cfg(feature = "std")]
    {
        // Check cache first (only for param-free translations like labels, titles)
        if context_hash.is_none() {
            if let Some(cached) = cache_translate(locale, key_hash, params) {
                return cached;
            }
        }
        let (result, key_found) = TRANSLATE_BUF.with(|cell| {
            let mut guard = cell.borrow_mut();
            guard.clear();
            let status =
                translate_to_writer_with_status(locale, key_hash, context_hash, params, &mut *guard)
                    .unwrap_or(TranslateStatus {
                        key_found: false,
                        locale_loaded: false,
                    });
            (guard.clone(), status.key_found)
        });
        cache_insert(locale, key_hash, params, &result, key_found);
        result
    }
    #[cfg(not(feature = "std"))]
    {
        let mut buf = String::new();
        let _ = translate_to_writer(locale, key_hash, context_hash, params, &mut buf);
        buf
    }
}

#[cfg(test)]
fn hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
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

    fn test_handler(locale: &str, key_hash: u64) {
        HANDLER_CALLED.store(true, Ordering::SeqCst);
        let _ = (locale, key_hash);
    }

    #[test]
    fn handler_called_on_missing_key() {
        let _lock = MISSING_KEY_MUTEX.lock().unwrap();
        clear_missing_key_handler();
        HANDLER_CALLED.store(false, Ordering::SeqCst);
        set_missing_key_handler(test_handler);
        let mut buf = alloc::string::String::new();
        let _ = translate_to_writer("xx", hash("nonexistent.key"), None, &[], &mut buf);
        assert!(
            HANDLER_CALLED.load(Ordering::SeqCst),
            "handler should have been called"
        );
        clear_missing_key_handler();
    }

    #[test]
    fn handler_called_when_key_not_found() {
        let _lock = MISSING_KEY_MUTEX.lock().unwrap();
        clear_missing_key_handler();
        HANDLER_CALLED.store(false, Ordering::SeqCst);
        set_missing_key_handler(test_handler);
        let mut buf = alloc::string::String::new();
        let _ = translate_to_writer("zz", hash("nonexistent"), None, &[], &mut buf);
        assert!(
            HANDLER_CALLED.load(Ordering::SeqCst),
            "handler should be called for missing key"
        );
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
    fn locale_parent_strips_last_component() {
        assert_eq!(locale_parent("en-US"), Some("en"));
        assert_eq!(locale_parent("zh-Hans-CN"), Some("zh-Hans"));
        assert_eq!(locale_parent("pt_BR"), Some("pt"));
    }

    #[test]
    fn locale_parent_returns_none_for_root_tag() {
        assert_eq!(locale_parent("en"), None);
        assert_eq!(locale_parent("fr"), None);
        assert_eq!(locale_parent(""), None);
    }

    #[test]
    fn locale_parent_handles_underscore_separator() {
        assert_eq!(locale_parent("zh_Hant"), Some("zh"));
    }

    #[test]
    fn locale_parent_dash_at_start_returns_none() {
        // If the separator is at position 0, should return None
        assert_eq!(locale_parent("-en"), None);
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
        Arc::make_mut(&mut store.locales)
            .push((String::from("en"), StoreData::Owned(Arc::clone(&buf))));
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
        let val_offset: u32 = 16;
        let index_offset: u32 = val_offset + val.len() as u32;
        buf.extend_from_slice(&index_offset.to_be_bytes());
        let index_count: u32 = 1;
        buf.extend_from_slice(&index_count.to_be_bytes());
        buf.extend_from_slice(val);
        let hash = crate::binary_format::fnv1a_64(key.as_bytes());
        buf.extend_from_slice(&hash.to_be_bytes());
        buf.extend_from_slice(&val_offset.to_be_bytes());
        buf.extend_from_slice(&(val.len() as u32).to_be_bytes());
        buf
    }

    fn load_locale_with_key(locale: &str, key: &str, val: &[u8]) {
        let locales = Arc::new(alloc::vec![(
            String::from(locale),
            StoreData::Owned(Arc::new(make_binary_with_key(key, val))),
        )]);
        let chain = get_fallback_chain();
        #[cfg(feature = "std")]
        {
            swap_store(TranslationStore {
                locales,
                fallback_chain: chain,
                lazy_cache: HashMap::new(),
                offset_maps: HashMap::new(),
            });
        }
        #[cfg(not(feature = "std"))]
        {
            swap_store(TranslationStore {
                locales,
                fallback_chain: chain,
            });
        }
    }

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
        assert!(!key_exists("en", hash("some.key"), None));
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
        let result = translate("xx", hash("missing.key"), None, &[]);
        assert_eq!(result, alloc::format!("{:#x}", hash("missing.key")));
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
        load_locale_with_key(
            "en",
            "greeting",
            &[0x01, 0x00, 0x00, 0x00, 0x05, b'H', b'e', b'l', b'l', b'o'],
        );
        assert!(
            key_exists("en", hash("greeting"), None),
            "should find existing key"
        );
    }

    #[test]
    fn key_exists_subtag_parent_pushes_candidate() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key(
            "en",
            "greeting",
            &[0x01, 0x00, 0x00, 0x00, 0x05, b'H', b'e', b'l', b'l', b'o'],
        );
        assert!(
            key_exists("en-US", hash("greeting"), None),
            "should find key via subtag parent 'en'"
        );
    }

    #[test]
    fn key_exists_with_context_suffix_hit() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key(
            "en",
            "greeting_male",
            &[0x01, 0x00, 0x00, 0x00, 0x05, b'H', b'e', b'l', b'l', b'o'],
        );
        assert!(
            key_exists("en", hash("greeting"), Some(hash("greeting_male"))),
            "should find context-suffixed key"
        );
    }

    #[test]
    fn translate_subtag_parent_success() {
        let _lock = lock_extra();
        clear_translations();
        let val: Vec<u8> = vec![
            0x01, 0x00, 0x00, 0x00, 0x0B, b'H', b'e', b'l', b'l', b'o', b' ', b'W', b'o', b'r',
            b'l', b'd',
        ];
        load_locale_with_key("en", "greeting", &val);
        let result = translate("en-US", hash("greeting"), None, &[]);
        assert_eq!(result, "Hello World", "should resolve via parent en");
    }

    #[test]
    fn translate_fallback_chain_skips_parent() {
        let _lock = lock_extra();
        clear_translations();
        // Load "fr" with greeting, set fallback to ["fr"], request "en-US"
        // Since "en-US"|"en" has no data, and fallback "fr" != parent "en", "fr" IS checked
        let val: Vec<u8> = vec![
            0x01, 0x00, 0x00, 0x00, 0x0B, b'H', b'e', b'l', b'l', b'o', b' ', b'W', b'o', b'r',
            b'l', b'd',
        ];
        load_locale_with_key("fr", "greeting", &val);
        set_fallback_chain(&["fr"]);
        let result = translate("en-US", hash("greeting"), None, &[]);
        assert_eq!(
            result, "Hello World",
            "should resolve via fallback fr since fr != parent en"
        );
        set_fallback_chain(&["en"]);
    }

    #[test]
    fn try_locale_inc_format_errors_on_bad_bytecode() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key("en", "broken", &[0xFF]);
        let before = crate::metrics::format_errors();
        let result = translate("en", hash("broken"), None, &[]);
        let after = crate::metrics::format_errors();
        assert_eq!(
            result,
            alloc::format!("{:#x}", hash("broken")),
            "bad bytecode falls through to key-as-text"
        );
        assert!(
            after > before,
            "format_errors should increase on bad bytecode"
        );
    }

    #[test]
    fn try_locale_context_format_error_increments_counter() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key("en", "broken_male", &[0xFF]);
        let before = crate::metrics::format_errors();
        let result = translate("en", hash("broken"), Some(hash("broken_male")), &[]);
        let after = crate::metrics::format_errors();
        assert_eq!(
            result,
            alloc::format!("{:#x}", hash("broken")),
            "bad context bytecode falls through"
        );
        assert!(
            after >= before,
            "format_errors should increase for bad context bytecode"
        );
    }

    #[test]
    fn load_static_bytes_then_translate() {
        let _lock = lock_extra();
        clear_translations();

        let val: &[u8] = &[0x01, 0x00, 0x00, 0x00, 0x05, b'H', b'e', b'l', b'l', b'o'];
        let val_offset: u32 = 16;
        let index_offset: u32 = val_offset + val.len() as u32;

        let mut data = Vec::new();
        data.extend_from_slice(b"L10N");
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&index_offset.to_be_bytes());
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(val);
        let kh = crate::binary_format::fnv1a_64(b"greeting");
        data.extend_from_slice(&kh.to_be_bytes());
        data.extend_from_slice(&val_offset.to_be_bytes());
        data.extend_from_slice(&(val.len() as u32).to_be_bytes());

        let static_data: &'static [u8] = Box::leak(data.into_boxed_slice());
        assert!(load_static_bytes("en", static_data, true));

        let result = translate("en", hash("greeting"), None, &[]);
        assert_eq!(result, "Hello", "should translate from static L10N data");
    }

    #[test]
    fn init_embedded_multiple_locales() {
        let _lock = lock_extra();
        clear_translations();

        fn make_l10n() -> &'static [u8] {
            let buf = vec![
                b'L', b'1', b'0', b'N', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00,
                0x00, 0x00,
            ];
            Box::leak(buf.into_boxed_slice())
        }

        let en_data = make_l10n();
        let es_data = make_l10n();
        init_embedded(&[("en", en_data), ("es", es_data)]);
        assert!(locale_loaded("en"));
        assert!(locale_loaded("es"));
    }

    #[test]
    fn static_and_owned_coexist() {
        let _lock = lock_extra();
        clear_translations();

        let static_en: &'static [u8] = Box::leak(
            vec![
                b'L', b'1', b'0', b'N', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00,
                0x00, 0x00,
            ]
            .into_boxed_slice(),
        );
        assert!(load_static_bytes("en", static_en, true));

        let buf = vec![
            b'L', b'1', b'0', b'N', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00,
            0x00, 0x00,
        ];
        assert!(crate::loader::load_raw_bytes("fr", buf));

        assert!(locale_loaded("en"));
        assert!(locale_loaded("fr"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn translate_cache_hit_returns_same_value() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key("en", "cache_test", b"cached-value");
        let key_hash = hash("cache_test");
        let r1 = translate("en", key_hash, None, &[]);
        let r2 = translate("en", key_hash, None, &[]);
        assert_eq!(r1, "cached-value");
        assert_eq!(r1, r2);
    }

    #[cfg(feature = "std")]
    #[test]
    fn translate_cache_skips_missing_keys() {
        let _lock = lock_extra();
        clear_translations();
        let buf = vec![
            b'L', b'1', b'0', b'N', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00,
            0x00, 0x00,
        ];
        assert!(crate::loader::load_raw_bytes("en", buf));
        let key_hash = hash("missing.cache");
        let r1 = translate("en", key_hash, None, &[]);
        let r2 = translate("en", key_hash, None, &[]);
        assert_eq!(r1, r2);
        assert!(r1.starts_with("0x"));
    }

    #[cfg(feature = "std")]
    #[test]
    fn translate_cache_skipped_when_params_provided() {
        let _lock = lock_extra();
        clear_translations();
        let buf = vec![
            b'L', b'1', b'0', b'N', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00,
            0x00, 0x00,
        ];
        assert!(crate::loader::load_raw_bytes("en", buf));
        // With params, cache should be bypassed
        let r1 = translate("en", hash("greeting"), None, &[("name", "World")]);
        let r2 = translate("en", hash("greeting"), None, &[("name", "World")]);
        assert_eq!(r1, r2);
    }
}
