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

/// Fast cache key: store id (`0` = global), locale hash, key hash.
#[cfg(feature = "std")]
type FastTranslateCacheKey = (u32, u64, u64);

/// Full cache key: store id, locale hash, key hash, context (`u64::MAX` = none), params hash.
#[cfg(feature = "std")]
type TranslateCacheKey = (u32, u64, u64, u64, u64);

#[cfg(feature = "std")]
thread_local! {
    static TRANSLATE_CACHE_FAST: RefCell<HashMap<FastTranslateCacheKey, String>> =
        RefCell::new(HashMap::new());
    static TRANSLATE_CACHE_FAST_ORDER: RefCell<VecDeque<FastTranslateCacheKey>> =
        const { RefCell::new(VecDeque::new()) };
    static TRANSLATE_CACHE: RefCell<HashMap<TranslateCacheKey, Arc<str>>> =
        RefCell::new(HashMap::new());
    static TRANSLATE_CACHE_ORDER: RefCell<VecDeque<TranslateCacheKey>> =
        const { RefCell::new(VecDeque::new()) };
}

/// Global store-mutation generation. Bumped on every locale load, clear, or OTA
/// swap so that per-thread translate caches on OTHER threads can detect the
/// change and drop stale entries (thread-locals can only be cleared by their
/// own thread).
#[cfg(feature = "std")]
static STORE_GENERATION: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

// Generation this thread's translate caches were last valid at.
#[cfg(feature = "std")]
thread_local! {
    static CACHE_GENERATION: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

/// Marks all per-thread translate caches stale, on every thread.
#[cfg(feature = "std")]
pub(crate) fn bump_store_generation() {
    STORE_GENERATION.fetch_add(1, core::sync::atomic::Ordering::Release);
}

/// Current store-mutation generation. Bindings that keep their own translate
/// caches (FFI, WASM) must record this at fill time and treat the entry as
/// stale when it no longer matches.
#[cfg(feature = "std")]
pub fn store_generation() -> u64 {
    STORE_GENERATION.load(core::sync::atomic::Ordering::Acquire)
}

/// Clears this thread's translate caches if the store changed since they were
/// last used. Must run before any cache lookup.
#[cfg(feature = "std")]
fn sync_translate_cache_generation() {
    let current = STORE_GENERATION.load(core::sync::atomic::Ordering::Acquire);
    CACHE_GENERATION.with(|generation| {
        if generation.get() != current {
            generation.set(current);
            TRANSLATE_CACHE_FAST.with(|cell| cell.borrow_mut().clear());
            TRANSLATE_CACHE_FAST_ORDER.with(|cell| cell.borrow_mut().clear());
            TRANSLATE_CACHE.with(|cell| cell.borrow_mut().clear());
            TRANSLATE_CACHE_ORDER.with(|cell| cell.borrow_mut().clear());
        }
    });
}

/// FNV-1a composite hash of interpolation parameters (shared by core, FFI, and WASM).
#[cfg(feature = "std")]
pub fn hash_params(params: &[(&str, &str)]) -> u64 {
    if params.is_empty() {
        return 0;
    }
    let mut h = 0xcbf29ce484222325u64;
    for (key, value) in params {
        h ^= crate::binary_format::fnv1a_64(key.as_bytes());
        h = h.wrapping_mul(0x100000001b3);
        h ^= crate::binary_format::fnv1a_64(value.as_bytes());
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[cfg(feature = "std")]
fn translate_cache_key(
    store_id: u32,
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
) -> TranslateCacheKey {
    (
        store_id,
        crate::binary_format::fnv1a_64(locale.as_bytes()),
        key_hash,
        context_hash.unwrap_or(u64::MAX),
        hash_params(params),
    )
}

#[cfg(feature = "std")]
fn cache_translate_fast(store_id: u32, locale_hash: u64, key_hash: u64) -> Option<String> {
    TRANSLATE_CACHE_FAST.with(|cell| {
        let cache = cell.borrow();
        cache.get(&(store_id, locale_hash, key_hash)).cloned()
    })
}

#[cfg(feature = "std")]
fn cache_insert_fast(store_id: u32, locale_hash: u64, key_hash: u64, result: &str) {
    let cache_key = (store_id, locale_hash, key_hash);
    TRANSLATE_CACHE_FAST.with(|cell| {
        let mut cache = cell.borrow_mut();
        TRANSLATE_CACHE_FAST_ORDER.with(|order_cell| {
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
fn cache_translate_full(
    store_id: u32,
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
) -> Option<String> {
    let cache_key = translate_cache_key(store_id, locale, key_hash, context_hash, params);
    TRANSLATE_CACHE.with(|cell| {
        let cache = cell.borrow();
        cache.get(&cache_key).map(|text| text.to_string())
    })
}

#[cfg(feature = "std")]
fn cache_insert_full(
    store_id: u32,
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
    result: Arc<str>,
) {
    let cache_key = translate_cache_key(store_id, locale, key_hash, context_hash, params);
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
            cache.insert(cache_key, result);
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
pub(crate) type LazyDecompressCache = HashMap<u64, Arc<OnceLock<(Vec<u8>, OffsetMap)>>>;
/// Per-locale O(1) offset cache: maps locale_hash to key-hash-based (offset, length) pairs.
#[cfg(feature = "std")]
pub(crate) type OffsetMap = Arc<HashMap<u64, (u32, u32)>>;
/// Loaded namespace names per locale hash (modular bundles).
#[cfg(feature = "std")]
type LoadedNamespacesMap = Arc<HashMap<u64, Arc<[Arc<str>]>>>;

/// Returns a mutable lazy-cache map, allocating `Arc` storage on first use.
#[cfg(feature = "std")]
pub(crate) fn lazy_cache_mut(
    cache: &mut Option<Arc<LazyDecompressCache>>,
) -> &mut LazyDecompressCache {
    match cache {
        Some(arc) => Arc::make_mut(arc),
        None => {
            *cache = Some(Arc::new(HashMap::new()));
            Arc::make_mut(cache.as_mut().expect("lazy_cache just initialized"))
        }
    }
}

/// Returns a mutable offset-map table, allocating `Arc` storage on first use.
#[cfg(feature = "std")]
pub(crate) fn offset_maps_mut(
    maps: &mut Option<Arc<HashMap<u64, OffsetMap>>>,
) -> &mut HashMap<u64, OffsetMap> {
    match maps {
        Some(arc) => Arc::make_mut(arc),
        None => {
            *maps = Some(Arc::new(HashMap::new()));
            Arc::make_mut(maps.as_mut().expect("offset_maps just initialized"))
        }
    }
}

/// Fine-grained COW upsert: rebuilds the sorted locale vector sharing unchanged `Arc<StoreData>` entries.
pub(crate) fn upsert_locale(
    locales: &mut Arc<Vec<(String, Arc<StoreData>)>>,
    locale: String,
    data: StoreData,
) {
    let data_arc = Arc::new(data);
    let old = Arc::clone(locales);
    let pos = old.binary_search_by(|(loc, _)| loc.as_str().cmp(locale.as_str()));
    let mut new_vec = Vec::with_capacity(old.len() + pos.is_err() as usize);
    match pos {
        Ok(idx) => {
            for (i, (loc, sd)) in old.iter().enumerate() {
                if i == idx {
                    new_vec.push((locale.clone(), Arc::clone(&data_arc)));
                } else {
                    new_vec.push((loc.clone(), Arc::clone(sd)));
                }
            }
        }
        Err(idx) => {
            for (i, (loc, sd)) in old.iter().enumerate() {
                if i == idx {
                    new_vec.push((locale.clone(), Arc::clone(&data_arc)));
                }
                new_vec.push((loc.clone(), Arc::clone(sd)));
            }
            if idx == old.len() {
                new_vec.push((locale, data_arc));
            }
        }
    }
    *locales = Arc::new(new_vec);
}

/// Manages loaded localization packages: maps locale codes to their decompressed binary buffers.
/// Uses a sorted `Vec` for O(log n) binary-search lookup and cache-friendly O(n) clone.
/// `locales` is `Arc`-wrapped so `clone()` on the whole store is O(1) when locales don't change.
pub struct TranslationStore {
    /// Sorted vector of locale-to-buffer mappings (Arc for O(1) clone). Binary-search for lookup.
    pub locales: Arc<Vec<(String, Arc<StoreData>)>>,
    /// Ordered chain of fallback locale codes. The first match wins.
    pub fallback_chain: Arc<[Arc<str>]>,
    /// Per-locale lazy decompression cache. Key: locale_hash. `None` when empty (no reload caches).
    #[cfg(feature = "std")]
    pub lazy_cache: Option<Arc<LazyDecompressCache>>,
    /// Per-locale O(1) offset maps. Key: locale_hash. `None` when empty.
    #[cfg(feature = "std")]
    pub offset_maps: Option<Arc<HashMap<u64, OffsetMap>>>,
    /// Optional hash → key name table (`debug-keys` feature).
    #[cfg(all(feature = "std", feature = "debug-keys"))]
    pub debug_keys: Option<Arc<HashMap<u64, Arc<str>>>>,
    /// Loaded namespace names per locale hash (modular bundles).
    #[cfg(feature = "std")]
    pub loaded_namespaces: Option<LoadedNamespacesMap>,
}

impl Default for TranslationStore {
    fn default() -> Self {
        Self {
            locales: Arc::new(Vec::new()),
            fallback_chain: default_chain(),
            #[cfg(feature = "std")]
            lazy_cache: None,
            #[cfg(feature = "std")]
            offset_maps: None,
            #[cfg(all(feature = "std", feature = "debug-keys"))]
            debug_keys: None,
            #[cfg(feature = "std")]
            loaded_namespaces: None,
        }
    }
}

/// Cloned store state used by RCU writers (`load`, `clear`, `swap_store`).
#[cfg(feature = "std")]
pub(crate) struct StoreSnapshot {
    pub locales: Arc<Vec<(String, Arc<StoreData>)>>,
    pub fallback_chain: Arc<[Arc<str>]>,
    pub lazy_cache: Option<Arc<LazyDecompressCache>>,
    pub offset_maps: Option<Arc<HashMap<u64, OffsetMap>>>,
    #[cfg(feature = "debug-keys")]
    pub debug_keys: Option<Arc<HashMap<u64, Arc<str>>>>,
    pub loaded_namespaces: Option<LoadedNamespacesMap>,
}

#[cfg(feature = "std")]
pub(crate) fn store_snapshot(store: &TranslationStore) -> StoreSnapshot {
    StoreSnapshot {
        locales: Arc::clone(&store.locales),
        fallback_chain: Arc::clone(&store.fallback_chain),
        lazy_cache: store.lazy_cache.clone(),
        offset_maps: store.offset_maps.clone(),
        #[cfg(feature = "debug-keys")]
        debug_keys: store.debug_keys.clone(),
        loaded_namespaces: store.loaded_namespaces.clone(),
    }
}

#[cfg(feature = "std")]
pub(crate) fn build_store(snap: StoreSnapshot) -> TranslationStore {
    TranslationStore {
        locales: snap.locales,
        fallback_chain: snap.fallback_chain,
        lazy_cache: snap.lazy_cache,
        offset_maps: snap.offset_maps,
        #[cfg(feature = "debug-keys")]
        debug_keys: snap.debug_keys,
        loaded_namespaces: snap.loaded_namespaces,
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
        match self.locales[idx].1.as_ref() {
            StoreData::Owned(v) => Some(v.as_slice()),
            StoreData::Static(v, _) => Some(v),
            #[cfg(feature = "std")]
            StoreData::Lazy(compressed) => {
                let locale_hash = crate::binary_format::fnv1a_64(locale.as_bytes());
                // A missing cache entry or a corrupt payload must NOT panic at
                // translate time: return None so the fallback chain takes over.
                let entry = self.lazy_cache.as_ref().and_then(|c| c.get(&locale_hash))?;
                let (decompressed, _) = entry.get_or_init(|| {
                    match crate::pak::decompress_zstd_payload(compressed.as_slice()) {
                        Ok(output) => {
                            let offsets = if let Ok(reader) =
                                crate::binary_format::BinaryFormatReader::new(&output)
                            {
                                Arc::new(reader.to_offsets())
                            } else {
                                Arc::new(HashMap::new())
                            };
                            (output, offsets)
                        }
                        Err(_) => {
                            crate::metrics::inc_format_errors();
                            // Empty sentinel: a valid L10N buffer is never
                            // empty (16-byte header minimum).
                            (Vec::new(), Arc::new(HashMap::new()))
                        }
                    }
                });
                if decompressed.is_empty() {
                    return None;
                }
                Some(decompressed.as_slice())
            }
        }
    }
}

/// Atomically transforms the global store: snapshot → `f` → install, all under
/// the writer lock. Concurrent writers are serialized, so no writer can lose
/// another's changes. If `f` returns `Err`, the store is left unchanged.
pub fn update_store<F, E>(f: F) -> Result<(), E>
where
    F: FnOnce(&TranslationStore) -> Result<TranslationStore, E>,
{
    crate::store_cell::StoreCell::global().update(f)
}

/// Like [`update_store`], but for the store selected by `handle` (`None` = global).
#[cfg(feature = "std")]
pub fn update_store_for<F>(handle: Option<StoreHandle>, f: F) -> CoreResult<()>
where
    F: FnOnce(&TranslationStore) -> CoreResult<TranslationStore>,
{
    with_cell(handle, |cell| cell.update(f))?
}

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
pub fn read_store<F, R>(f: F) -> R
where
    F: FnOnce(&TranslationStore) -> R,
{
    #[cfg(all(not(feature = "std"), debug_assertions))]
    let _guard = ReentrancyGuard::new();

    crate::store_cell::StoreCell::global().read(f)
}

/// Swaps the current store with a new one and schedules the old store for reclamation.
/// Thread-safe with concurrent readers; writers are serialized internally.
pub fn swap_store(new_store: TranslationStore) {
    #[cfg(all(not(feature = "std"), debug_assertions))]
    assert_eq!(
        REENTRANCY_COUNTER.load(Ordering::Acquire),
        0,
        "Reentrancy detected: swap_store called within read_store"
    );

    crate::store_cell::StoreCell::global().swap(new_store);
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
        let _ = update_store::<_, core::convert::Infallible>(|store| {
            let mut snap = store_snapshot(store);
            snap.fallback_chain = new_chain;
            Ok(build_store(snap))
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

#[cfg(feature = "std")]
use crate::store_registry::{with_cell, StoreHandle};

#[cfg(feature = "std")]
fn store_id_from_handle(handle: Option<StoreHandle>) -> u32 {
    handle.map(|h| h.raw()).unwrap_or(0)
}

/// Reads the store for `handle` (`None` = global).
#[cfg(feature = "std")]
pub fn read_store_for<F, R>(handle: Option<StoreHandle>, f: F) -> CoreResult<R>
where
    F: FnOnce(&TranslationStore) -> R,
{
    with_cell(handle, |cell| cell.read(f))
}

/// Swaps the store for `handle` (`None` = global).
#[cfg(feature = "std")]
pub fn swap_store_for(handle: Option<StoreHandle>, new_store: TranslationStore) -> CoreResult<()> {
    with_cell(handle, |cell| {
        cell.swap(new_store);
    })
}

/// Sets the fallback locale chain on a scoped store.
#[cfg(feature = "std")]
pub fn set_fallback_chain_for_store(handle: StoreHandle, chain: &[&str]) -> CoreResult<()> {
    let arcs: Vec<Arc<str>> = chain.iter().map(|s| Arc::from(*s)).collect();
    let new_chain: Arc<[Arc<str>]> = Arc::from(arcs.into_boxed_slice());
    update_store_for(Some(handle), |store| {
        let mut snap = store_snapshot(store);
        snap.fallback_chain = new_chain;
        Ok(build_store(snap))
    })
}

/// Returns the fallback chain for a scoped store.
#[cfg(feature = "std")]
pub fn get_fallback_chain_for_store(handle: StoreHandle) -> Arc<[Arc<str>]> {
    read_store_for(Some(handle), |store| Arc::clone(&store.fallback_chain))
        .expect("valid store handle")
}

/// Clears all loaded translations for a scoped store (resets to default state).
#[cfg(feature = "std")]
pub fn clear_translations_for_store(handle: Option<StoreHandle>) -> CoreResult<()> {
    swap_store_for(handle, TranslationStore::default())?;
    notify_locale_changed_for_handle(handle, "*");
    Ok(())
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
        swap_store(build_store(StoreSnapshot {
            locales: Arc::new(Vec::new()),
            fallback_chain: chain,
            lazy_cache: None,
            offset_maps: None,
            #[cfg(feature = "debug-keys")]
            debug_keys: None,
            loaded_namespaces: None,
        }));
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
        .as_ref()
        .and_then(|c| c.get(&locale_hash))
        .and_then(|entry| entry.get().map(|(_, offsets)| offsets.as_ref()));
    store
        .offset_maps
        .as_ref()
        .and_then(|m| m.get(&locale_hash))
        .filter(|m| !m.is_empty())
        .map(|arc| arc.as_ref())
        .or(lazy_offsets)
}

fn hash_present_in_locale(store: &TranslationStore, locale: &str, key_hash: u64) -> bool {
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

/// Runs `f` over the fallback candidates for `locale` in resolution order —
/// the locale itself, its BCP-47 parent, then the configured chain (skipping
/// duplicates of the first two) — stopping at the first `true`.
///
/// Single source of truth for the resolution order: `key_exists` and translate
/// walk the exact same chain.
fn for_each_fallback_candidate(
    locale: &str,
    chain: &[Arc<str>],
    mut f: impl FnMut(&str) -> bool,
) -> bool {
    if f(locale) {
        return true;
    }
    let parent = locale_parent(locale).filter(|p| *p != locale);
    if let Some(parent) = parent {
        if f(parent) {
            return true;
        }
    }
    for fb in chain.iter() {
        let fb_str: &str = fb.as_ref();
        if fb_str == locale || Some(fb_str) == parent {
            continue;
        }
        if f(fb_str) {
            return true;
        }
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
    for_each_fallback_candidate(locale, chain, |candidate| {
        if let Some(ctx_hash) = context_hash {
            if hash_present_in_locale(store, candidate, ctx_hash) {
                return true;
            }
        }
        hash_present_in_locale(store, candidate, key_hash)
    })
}

/// Resolves and formats a translation, walking the fallback chain. Returns
/// `(key_found, primary_locale_loaded)` from a single store read — callers
/// previously did a separate `store.lookup(locale)` just for the loaded flag.
fn resolve_translate_in_store<W: core::fmt::Write>(
    store: &TranslationStore,
    chain: &[Arc<str>],
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
    writer: &mut W,
) -> (bool, bool) {
    let mut primary_loaded = false;
    let mut is_primary = true;
    let found = for_each_fallback_candidate(locale, chain, |candidate| {
        let buf = store.lookup(candidate);
        if is_primary {
            primary_loaded = buf.is_some();
            is_primary = false;
        }
        match buf {
            Some(buf) => try_locale(
                store,
                candidate,
                buf,
                key_hash,
                context_hash,
                params,
                writer,
            ),
            None => false,
        }
    });
    (found, primary_loaded)
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

/// Returns `true` when modular bundle `namespace` is loaded for `locale`.
#[cfg(feature = "std")]
pub fn namespace_loaded(locale: &str, namespace: &str) -> bool {
    read_store(|store| {
        let locale_hash = crate::binary_format::fnv1a_64(locale.as_bytes());
        store
            .loaded_namespaces
            .as_ref()
            .and_then(|m| m.get(&locale_hash))
            .is_some_and(|list| list.iter().any(|n| n.as_ref() == namespace))
    })
}

/// Resolves a key hash to its human-readable name (`debug-keys` compile feature).
#[cfg(all(feature = "std", feature = "debug-keys"))]
pub fn resolve_key_name(key_hash: u64) -> Option<Arc<str>> {
    read_store(|store| {
        store
            .debug_keys
            .as_ref()
            .and_then(|m| m.get(&key_hash).cloned())
    })
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
        let _ = update_store::<_, core::convert::Infallible>(|store| {
            let mut snap = store_snapshot(store);
            let offset_map = offset_maps_mut(&mut snap.offset_maps);
            let locale_hash = crate::binary_format::fnv1a_64(locale_str.as_bytes());
            let offset_arc = if let Ok(reader) = crate::binary_format::BinaryFormatReader::new(data)
            {
                Arc::new(reader.to_offsets())
            } else {
                Arc::new(HashMap::new())
            };
            offset_map.insert(locale_hash, offset_arc);
            #[cfg(feature = "debug-keys")]
            if let Ok(reader) = crate::binary_format::BinaryFormatReader::new(data) {
                let table = reader.debug_key_table();
                if !table.is_empty() {
                    let dk = snap
                        .debug_keys
                        .get_or_insert_with(|| Arc::new(HashMap::new()));
                    let map = Arc::make_mut(dk);
                    for (hash, name) in table {
                        map.insert(hash, Arc::from(name.as_str()));
                    }
                }
            }
            upsert_locale(
                &mut snap.locales,
                locale_str.to_string(),
                StoreData::Static(data, already_verified),
            );
            let ns_map = snap
                .loaded_namespaces
                .get_or_insert_with(|| Arc::new(HashMap::new()));
            Arc::make_mut(ns_map).remove(&locale_hash);
            Ok(build_store(snap))
        });
        emit_locale_changed(locale_str);
        true
    }
    #[cfg(not(feature = "std"))]
    {
        let (mut locales, fallback_chain) = read_store(|store| {
            (
                Arc::clone(&store.locales),
                Arc::clone(&store.fallback_chain),
            )
        });
        upsert_locale(
            &mut locales,
            locale_str.to_string(),
            StoreData::Static(data, already_verified),
        );
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

/// Boxed dynamic callback (std): `Mutex<Option<Arc<...>>>` instead of a raw
/// `AtomicPtr` — the previous scheme leaked the old callback on
/// re-registration and could drop the box while another thread was calling
/// through it (use-after-free). This is a cold path; a lock is fine.
#[cfg(feature = "std")]
type BoxedLocaleCallback = Arc<std::sync::Mutex<Box<dyn Fn(&str) + Send>>>;
#[cfg(feature = "std")]
static LOCALE_CHANGE_BOXED: std::sync::Mutex<Option<BoxedLocaleCallback>> =
    std::sync::Mutex::new(None);

/// Boxed dynamic callback (no_std, single-threaded): plain pointer slot.
#[cfg(not(feature = "std"))]
static LOCALE_CHANGE_BOXED: core::sync::atomic::AtomicPtr<()> =
    core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());

/// Registers a callback invoked when a locale is loaded or cleared.
pub fn on_locale_changed(callback: LocaleChangeFn) {
    LOCALE_CHANGE_CALLBACKS.store(callback as *mut (), core::sync::atomic::Ordering::Release);
}

/// Registers a boxed dynamic callback for WASM bindings.
/// Re-registration replaces (and drops) the previous callback.
pub fn on_locale_changed_boxed(callback: alloc::boxed::Box<dyn Fn(&str) + Send>) {
    #[cfg(feature = "std")]
    {
        *LOCALE_CHANGE_BOXED
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(std::sync::Mutex::new(callback)));
    }
    #[cfg(not(feature = "std"))]
    {
        let ptr = alloc::boxed::Box::into_raw(alloc::boxed::Box::new(callback));
        let old = LOCALE_CHANGE_BOXED.swap(ptr as *mut (), core::sync::atomic::Ordering::AcqRel);
        if !old.is_null() {
            // SAFETY: single-threaded no_std — no concurrent caller can hold
            // the old pointer.
            unsafe {
                drop(alloc::boxed::Box::from_raw(
                    old as *mut Box<dyn Fn(&str) + Send>,
                ));
            }
        }
    }
}

/// Removes all locale change callbacks.
pub fn clear_locale_changed_callbacks() {
    LOCALE_CHANGE_CALLBACKS.store(core::ptr::null_mut(), core::sync::atomic::Ordering::Release);
    #[cfg(feature = "std")]
    {
        *LOCALE_CHANGE_BOXED
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }
    #[cfg(not(feature = "std"))]
    {
        let old =
            LOCALE_CHANGE_BOXED.swap(core::ptr::null_mut(), core::sync::atomic::Ordering::AcqRel);
        if !old.is_null() {
            unsafe {
                drop(alloc::boxed::Box::from_raw(
                    old as *mut Box<dyn Fn(&str) + Send>,
                ));
            }
        }
    }
}

#[cfg(feature = "std")]
pub(crate) fn emit_locale_changed_for_store(_store_id: u32, _locale: &str) {
    // Translate caches are thread-local, so this thread cannot clear another
    // thread's entries directly. Bumping the global generation makes EVERY
    // thread (this one included) drop its caches on its next translate call.
    // Store mutations are rare; losing unrelated cached entries is acceptable.
    bump_store_generation();
}

fn invoke_locale_changed_callbacks(locale: &str) {
    let ptr = LOCALE_CHANGE_CALLBACKS.load(core::sync::atomic::Ordering::Acquire);
    if !ptr.is_null() {
        let f: LocaleChangeFn = unsafe { core::mem::transmute(ptr) };
        f(locale);
    }
    #[cfg(feature = "std")]
    {
        // Clone the Arc under the registry lock, call outside it: a concurrent
        // clear/re-register only drops the callback after this call returns.
        let callback = LOCALE_CHANGE_BOXED
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(callback) = callback {
            // try_lock: a callback that itself triggers a locale change must
            // not deadlock — the nested notification is skipped instead.
            if let Ok(guard) = callback.try_lock() {
                (guard)(locale);
            }
        }
    }
    #[cfg(not(feature = "std"))]
    {
        let boxed_ptr = LOCALE_CHANGE_BOXED.load(core::sync::atomic::Ordering::Acquire);
        if !boxed_ptr.is_null() {
            // SAFETY: single-threaded no_std — no concurrent clear can drop it.
            unsafe {
                (*(boxed_ptr as *mut Box<dyn Fn(&str) + Send>))(locale);
            }
        }
    }
}

/// Invalidates translate caches for `handle` and invokes locale-change callbacks.
#[cfg(feature = "std")]
pub(crate) fn notify_locale_changed_for_handle(handle: Option<StoreHandle>, locale: &str) {
    emit_locale_changed_for_store(store_id_from_handle(handle), locale);
    invoke_locale_changed_callbacks(locale);
}

pub(crate) fn emit_locale_changed(locale: &str) {
    #[cfg(feature = "std")]
    emit_locale_changed_for_store(0, locale);
    invoke_locale_changed_callbacks(locale);
}

// Scratch buffer for format_message_checked (committed to the caller's writer
// only on success).
#[cfg(feature = "std")]
thread_local! {
    static FORMAT_SCRATCH: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Formats `bytecode` into `writer`, committing output only when the WHOLE
/// message formats successfully. Formatting directly into the caller's writer
/// would leave partial text behind on a mid-message error, and the fallback
/// locale's translation (or the key-hash placeholder) would then be appended
/// after it, producing garbled concatenated output.
fn format_message_checked<W: core::fmt::Write>(
    bytecode: &[u8],
    locale: &str,
    params: &[(&str, &str)],
    writer: &mut W,
) -> bool {
    #[cfg(feature = "std")]
    {
        let done = FORMAT_SCRATCH.with(|cell| {
            // try_borrow_mut: a custom formatter callback may re-enter
            // translation; fall back to a fresh local buffer in that case.
            if let Ok(mut buf) = cell.try_borrow_mut() {
                buf.clear();
                if format_message(bytecode, locale, params, &mut *buf).is_ok() {
                    Some(writer.write_str(&buf).is_ok())
                } else {
                    Some(false)
                }
            } else {
                None
            }
        });
        if let Some(result) = done {
            return result;
        }
    }
    let mut buf = String::new();
    if format_message(bytecode, locale, params, &mut buf).is_ok() {
        writer.write_str(&buf).is_ok()
    } else {
        false
    }
}

/// Attempts to translate `key_hash` from `locale` in `store`, writing to `writer`.
/// Returns `true` if translation succeeded.
/// Slices `buf[off..off+len]` with overflow-safe bounds (u32 offsets can wrap
/// `usize` on 32-bit targets) and formats the entry, committing on success.
#[cfg(feature = "std")]
#[inline]
fn try_offset_entry<W: core::fmt::Write>(
    buf: &[u8],
    off: u32,
    len: u32,
    locale: &str,
    params: &[(&str, &str)],
    writer: &mut W,
) -> bool {
    let start = off as usize;
    let Some(end) = start.checked_add(len as usize) else {
        return false;
    };
    if end > buf.len() {
        return false;
    }
    format_message_checked(&buf[start..end], locale, params, writer)
}

#[inline]
// `store` is only read inside the `#[cfg(feature = "std")]` offset-map fast
// path below; the no_std fallback path doesn't need it.
#[cfg_attr(not(feature = "std"), allow(unused_variables))]
fn try_locale<W: core::fmt::Write>(
    store: &TranslationStore,
    locale: &str,
    buf: &[u8],
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
    writer: &mut W,
) -> bool {
    #[cfg(feature = "std")]
    {
        if let Some(map) = offset_map_for_locale(store, locale) {
            let mut used_offset_map = false;
            if let Some(ctx_hash) = context_hash {
                if let Some(&(off, len)) = map.get(&ctx_hash) {
                    used_offset_map = true;
                    if try_offset_entry(buf, off, len, locale, params, writer) {
                        return true;
                    }
                    crate::metrics::inc_format_errors();
                }
            }
            if let Some(&(off, len)) = map.get(&key_hash) {
                used_offset_map = true;
                if try_offset_entry(buf, off, len, locale, params, writer) {
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
                if format_message_checked(bytecode, locale, params, writer) {
                    return true;
                }
                crate::metrics::inc_format_errors();
            }
        }
        if let Some(bytecode) = reader.lookup(key_hash) {
            if format_message_checked(bytecode, locale, params, writer) {
                return true;
            }
            crate::metrics::inc_format_errors();
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
    #[cfg(feature = "tracing")]
    let _span =
        tracing::trace_span!("l10n4x.translate", locale = locale, key_hash = key_hash).entered();

    crate::metrics::inc_total_translations();

    #[cfg(all(feature = "std", debug_assertions))]
    {
        log::debug!("translate: locale={}, key_hash={:#x}", locale, key_hash);
    }

    let status = read_store(|store| {
        let (found, locale_loaded) = resolve_translate_in_store(
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
        crate::metrics::inc_cache_misses_for_locale(locale);
        call_missing_key_handler(locale, key_hash);
        let _ = core::write!(writer, "{:#x}", key_hash);
    }
    Ok(status)
}

/// Scoped variant of [`translate_to_writer_with_status`] — reads from `handle` (`None` = global).
#[cfg(feature = "std")]
pub fn translate_to_writer_with_status_for_store<W: core::fmt::Write>(
    handle: Option<StoreHandle>,
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
    writer: &mut W,
) -> CoreResult<TranslateStatus> {
    #[cfg(feature = "tracing")]
    let _span =
        tracing::trace_span!("l10n4x.translate", locale = locale, key_hash = key_hash).entered();

    crate::metrics::inc_total_translations();

    #[cfg(all(feature = "std", debug_assertions))]
    {
        log::debug!("translate: locale={}, key_hash={:#x}", locale, key_hash);
    }

    let status = read_store_for(handle, |store| {
        let (found, locale_loaded) = resolve_translate_in_store(
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
    })?;

    if status.key_found {
        crate::metrics::inc_cache_hits();
    } else {
        crate::metrics::inc_cache_misses_for_locale(locale);
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
        translate_for_store(None, locale, key_hash, context_hash, params)
    }
    #[cfg(not(feature = "std"))]
    {
        let mut buf = String::new();
        let _ = translate_to_writer(locale, key_hash, context_hash, params, &mut buf);
        buf
    }
}

/// Translates using the store identified by `handle` (`None` = process-global store).
#[cfg(feature = "std")]
pub fn translate_for_store(
    handle: Option<StoreHandle>,
    locale: &str,
    key_hash: u64,
    context_hash: Option<u64>,
    params: &[(&str, &str)],
) -> String {
    sync_translate_cache_generation();
    let store_id = store_id_from_handle(handle);
    let locale_hash = crate::binary_format::fnv1a_64(locale.as_bytes());
    let use_fast_cache = context_hash.is_none() && params.is_empty();
    if use_fast_cache {
        if let Some(cached) = cache_translate_fast(store_id, locale_hash, key_hash) {
            return cached;
        }
    } else if let Some(cached) =
        cache_translate_full(store_id, locale, key_hash, context_hash, params)
    {
        return cached;
    }
    let (result, key_found) = TRANSLATE_BUF.with(|cell| {
        let mut guard = cell.borrow_mut();
        guard.clear();
        let status = translate_to_writer_with_status_for_store(
            handle,
            locale,
            key_hash,
            context_hash,
            params,
            &mut *guard,
        )
        .unwrap_or(TranslateStatus {
            key_found: false,
            locale_loaded: false,
        });
        // clone() instead of mem::take: taking would zero the buffer's
        // capacity and force it to re-grow on every call.
        (guard.clone(), status.key_found)
    });
    if key_found {
        if use_fast_cache {
            cache_insert_fast(store_id, locale_hash, key_hash, &result);
        } else {
            cache_insert_full(
                store_id,
                locale,
                key_hash,
                context_hash,
                params,
                Arc::<str>::from(result.as_str()),
            );
        }
    }
    result
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
        upsert_locale(
            &mut store.locales,
            String::from("en"),
            StoreData::Owned(Arc::clone(&buf)),
        );
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

    fn make_binary_with_keys(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(b"L10N");
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf.extend_from_slice(&(entries.len() as u32).to_be_bytes());

        let mut sorted = entries.to_vec();
        sorted.sort_by_key(|(key, _)| hash(key));

        let mut index_records = Vec::with_capacity(sorted.len());
        for (key, val) in sorted {
            let val_offset = buf.len() as u32;
            buf.extend_from_slice(val);
            index_records.push((hash(key), val_offset, val.len() as u32));
        }

        let index_offset = buf.len() as u32;
        buf[8..12].copy_from_slice(&index_offset.to_be_bytes());
        for (key_hash, val_offset, val_len) in index_records {
            buf.extend_from_slice(&key_hash.to_be_bytes());
            buf.extend_from_slice(&val_offset.to_be_bytes());
            buf.extend_from_slice(&val_len.to_be_bytes());
        }
        buf
    }

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
            Arc::new(StoreData::Owned(Arc::new(make_binary_with_key(key, val)))),
        )]);
        let chain = get_fallback_chain();
        #[cfg(feature = "std")]
        {
            swap_store(TranslationStore {
                locales,
                fallback_chain: chain,
                lazy_cache: None,
                offset_maps: None,
                #[cfg(feature = "debug-keys")]
                debug_keys: None,
                loaded_namespaces: None,
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
    fn translate_cache_hit_with_params() {
        let _lock = lock_extra();
        clear_translations();
        load_locale_with_key(
            "en",
            "greeting",
            &[
                0x01, 0, 0, 0, 3, b'H', b'i', b' ', 0x02, 0, 0, 0, 4, b'n', b'a', b'm', b'e',
            ],
        );
        let key_hash = hash("greeting");
        let params = &[("name", "World")];
        let r1 = translate("en", key_hash, None, params);
        let r2 = translate("en", key_hash, None, params);
        assert_eq!(r1, "Hi World");
        assert_eq!(r1, r2);
    }

    #[cfg(feature = "std")]
    #[test]
    fn translate_cache_hit_with_context() {
        let _lock = lock_extra();
        clear_translations();
        let buf = make_binary_with_keys(&[
            ("friend", b"friend-default"),
            ("friend_male", b"friend-male"),
        ]);
        assert!(crate::loader::load_raw_bytes("en", buf));
        let key_hash = hash("friend");
        let ctx_hash = hash("friend_male");
        let r1 = translate("en", key_hash, Some(ctx_hash), &[]);
        let r2 = translate("en", key_hash, Some(ctx_hash), &[]);
        assert_eq!(r1, "friend-male");
        assert_eq!(r1, r2);
    }

    /// A message that fails to format halfway must not leave partial text in
    /// the output before the fallback locale's translation is appended.
    #[cfg(feature = "std")]
    #[test]
    fn failed_format_does_not_leak_partial_output() {
        let _lock = lock_extra();
        clear_translations();
        // Bytecode: text node "Ho" followed by a truncated variable opcode —
        // formats "Ho" then errors.
        let mut broken = vec![0x01, 0, 0, 0, 2, b'H', b'o'];
        broken.extend_from_slice(&[0x02, 0, 0, 0, 40]); // var len 40, no bytes
        assert!(crate::loader::load_raw_bytes(
            "es",
            make_binary_with_key("greet", &broken)
        ));
        assert!(crate::loader::load_raw_bytes(
            "en",
            make_binary_with_key("greet", b"Hello")
        ));
        set_fallback_chain(&["en"]);
        let result = translate("es", hash("greet"), None, &[]);
        assert_eq!(
            result, "Hello",
            "partial output from the broken es message leaked into the result"
        );
    }

    /// Two concurrent loads of different locales must both survive: the
    /// snapshot→mutate→swap sequence is serialized by the writer lock, so
    /// neither writer can base its new store on a stale snapshot.
    #[cfg(feature = "std")]
    #[test]
    fn concurrent_loads_do_not_lose_locales() {
        let _lock = lock_extra();
        for round in 0..20 {
            clear_translations();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let b2 = std::sync::Arc::clone(&barrier);
            let worker = std::thread::spawn(move || {
                b2.wait();
                assert!(crate::loader::load_raw_bytes(
                    "aa",
                    make_binary_with_key("k", b"a")
                ));
            });
            barrier.wait();
            assert!(crate::loader::load_raw_bytes(
                "bb",
                make_binary_with_key("k", b"b")
            ));
            worker.join().unwrap();
            assert!(
                locale_loaded("aa") && locale_loaded("bb"),
                "concurrent load lost a locale (round {round})"
            );
        }
    }

    /// Translate caches are thread-local; a reload on one thread must
    /// invalidate the caches of every other thread (via the store generation).
    #[cfg(feature = "std")]
    #[test]
    fn translate_cache_invalidated_across_threads() {
        let _lock = lock_extra();
        clear_translations();
        assert!(crate::loader::load_raw_bytes(
            "en",
            make_binary_with_key("xthread", b"old-text")
        ));
        let key_hash = hash("xthread");

        let (task_tx, task_rx) = std::sync::mpsc::channel::<()>();
        let (result_tx, result_rx) = std::sync::mpsc::channel::<String>();
        let worker = std::thread::spawn(move || {
            while task_rx.recv().is_ok() {
                let _ = result_tx.send(translate("en", key_hash, None, &[]));
            }
        });

        task_tx.send(()).unwrap();
        assert_eq!(result_rx.recv().unwrap(), "old-text");

        // Reload with different text from THIS thread; the worker's
        // thread-local cache must not survive it.
        assert!(crate::loader::load_raw_bytes(
            "en",
            make_binary_with_key("xthread", b"new-text")
        ));

        task_tx.send(()).unwrap();
        assert_eq!(
            result_rx.recv().unwrap(),
            "new-text",
            "worker thread served stale cached text after reload"
        );
        drop(task_tx);
        worker.join().unwrap();
    }
}
