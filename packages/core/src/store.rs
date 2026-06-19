extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicPtr, AtomicUsize, Ordering, AtomicBool};
use core::cell::UnsafeCell;
use crate::binary_format::BinaryFormatReader;
use crate::formatter::format_message;

pub struct TranslationStore {
    // List of (locale, decrypted binary buffer)
    pub locales: Vec<(String, Vec<u8>)>,
}

impl TranslationStore {
    pub fn lookup(&self, locale: &str) -> Option<&[u8]> {
        for (loc, buf) in &self.locales {
            if loc == locale {
                return Some(buf);
            }
        }
        None
    }
}

// Global active readers count for quiescent state detection
static READERS: AtomicUsize = AtomicUsize::new(0);

// Global store pointer
static STORE: AtomicPtr<TranslationStore> = AtomicPtr::new(core::ptr::null_mut());

// Fallback locale pointer
static FALLBACK_LOCALE: AtomicPtr<String> = AtomicPtr::new(core::ptr::null_mut());

// A simple spin-lock protected vector for retired pointers to be cleaned up
struct SpinMutex<T> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Sync for SpinMutex<T> {}
unsafe impl<T: Send> Send for SpinMutex<T> {}

impl<T> SpinMutex<T> {
    const fn new(val: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(val),
        }
    }

    fn lock(&self) -> SpinMutexGuard<'_, T> {
        while self.lock.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        SpinMutexGuard { mutex: self }
    }
}

struct SpinMutexGuard<'a, T> {
    mutex: &'a SpinMutex<T>,
}

impl<'a, T> core::ops::Deref for SpinMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T> core::ops::DerefMut for SpinMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T> Drop for SpinMutexGuard<'a, T> {
    fn drop(&mut self) {
        self.mutex.lock.store(false, Ordering::Release);
    }
}

struct RetiredStore(*mut TranslationStore);
unsafe impl Send for RetiredStore {}
unsafe impl Sync for RetiredStore {}

static RETIRED_STORES: SpinMutex<Vec<RetiredStore>> = SpinMutex::new(Vec::new());

/// Safely executes a function with a reference to the current TranslationStore
pub fn read_store<F, R>(f: F) -> R
where
    F: FnOnce(&TranslationStore) -> R,
{
    READERS.fetch_add(1, Ordering::SeqCst);
    let ptr = STORE.load(Ordering::SeqCst);
    let res = if !ptr.is_null() {
        unsafe { f(&*ptr) }
    } else {
        let empty = TranslationStore { locales: Vec::new() };
        f(&empty)
    };
    READERS.fetch_sub(1, Ordering::SeqCst);
    res
}

/// Swaps the current store with a new one and queues the old store for deletion
pub fn swap_store(new_store: TranslationStore) {
    let new_ptr = Box::into_raw(Box::new(new_store));
    let old_ptr = STORE.swap(new_ptr, Ordering::SeqCst);
    if !old_ptr.is_null() {
        let mut retired = RETIRED_STORES.lock();
        retired.push(RetiredStore(old_ptr));
    }
    // Attempt to reclaim retired stores if there are no active readers
    try_reclaim();
}

/// Reclaims memory for any retired stores if the reader count is 0
pub fn try_reclaim() {
    if READERS.load(Ordering::SeqCst) == 0 {
        if let Some(retired) = RETIRED_STORES.take_retired() {
            for item in retired {
                unsafe {
                    let _ = Box::from_raw(item.0);
                }
            }
        }
    }
}

// Extends RETIRED_STORES with a helper to extract vector under lock
impl SpinMutex<Vec<RetiredStore>> {
    fn take_retired(&self) -> Option<Vec<RetiredStore>> {
        let _guard = self.lock();
        let vec_ref = unsafe { &mut *self.data.get() };
        if vec_ref.is_empty() {
            None
        } else {
            Some(core::mem::take(vec_ref))
        }
    }
}

/// Returns the currently configured fallback locale (defaults to "en").
pub fn get_fallback_locale() -> String {
    let ptr = FALLBACK_LOCALE.load(Ordering::Acquire);
    if !ptr.is_null() {
        unsafe { (*ptr).clone() }
    } else {
        "en".to_string()
    }
}

/// Sets the global fallback locale (defaults to "en").
pub fn set_fallback_locale(locale_str: &str) -> bool {
    let new_ptr = Box::into_raw(Box::new(locale_str.to_string()));
    let old_ptr = FALLBACK_LOCALE.swap(new_ptr, Ordering::AcqRel);
    if !old_ptr.is_null() {
        // Safe to free when reader count is 0, spin-wait briefly
        while READERS.load(Ordering::SeqCst) > 0 {
            core::hint::spin_loop();
        }
        unsafe {
            let _ = Box::from_raw(old_ptr);
        }
    }
    true
}

/// Clears all loaded translations.
pub fn clear_translations() {
    swap_store(TranslationStore { locales: Vec::new() });
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
        writer.write_str(key).map_err(|_| "Failed to write key fallback")?;
        Ok(())
    }
}
