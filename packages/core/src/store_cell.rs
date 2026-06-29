//! Lock-free RCU cell wrapping one `TranslationStore`.

use crate::store::TranslationStore;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicPtr, Ordering};

#[cfg(feature = "std")]
use std::sync::Mutex;

/// Lock-free RCU cell holding a single [`TranslationStore`].
pub struct StoreCell {
    ptr: AtomicPtr<TranslationStore>,
    #[cfg(feature = "std")]
    write_mutex: Mutex<()>,
}

impl StoreCell {
    /// Creates a cell with `initial` as the active store.
    pub fn new(initial: TranslationStore) -> Self {
        let boxed = Box::into_raw(Box::new(initial));
        Self {
            ptr: AtomicPtr::new(boxed),
            #[cfg(feature = "std")]
            write_mutex: Mutex::new(()),
        }
    }

    /// Returns the process-global store cell (null pointer until first swap).
    pub fn global() -> &'static StoreCell {
        static GLOBAL: StoreCell = StoreCell {
            ptr: AtomicPtr::new(core::ptr::null_mut()),
            #[cfg(feature = "std")]
            write_mutex: Mutex::new(()),
        };
        &GLOBAL
    }

    /// Reads the current store under an epoch pin (`std`) or unpinned (`no_std`).
    #[cfg(feature = "std")]
    pub fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&TranslationStore) -> R,
    {
        let _guard = crossbeam_epoch::pin();
        self.read_unpinned(f)
    }

    /// Reads the current store (single-threaded `no_std`).
    #[cfg(not(feature = "std"))]
    pub fn read<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&TranslationStore) -> R,
    {
        self.read_unpinned(f)
    }

    fn read_unpinned<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&TranslationStore) -> R,
    {
        let ptr = self.ptr.load(Ordering::Acquire);
        if ptr.is_null() {
            let empty = TranslationStore::default();
            f(&empty)
        } else {
            // SAFETY: RCU — old pointer kept alive until epoch flush / no_std single-thread.
            unsafe { f(&*ptr) }
        }
    }

    /// Runs `f` while holding the writer mutex (serializes with [`Self::swap`]).
    #[cfg(feature = "std")]
    pub fn with_write<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = self.write_mutex.lock().unwrap_or_else(|p| p.into_inner());
        f()
    }

    /// Replaces the active store and schedules the previous one for reclamation.
    pub fn swap(&self, new_store: TranslationStore) {
        #[cfg(feature = "std")]
        let _guard = self.write_mutex.lock().unwrap_or_else(|p| p.into_inner());
        let new_ptr = Box::into_raw(Box::new(new_store));
        let old_ptr = self.ptr.swap(new_ptr, Ordering::SeqCst);
        if !old_ptr.is_null() {
            crate::reclaim::schedule_drop(old_ptr);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::TranslationStore;

    #[test]
    fn swap_and_read_roundtrip() {
        let cell = StoreCell::new(TranslationStore::default());
        let mut snap = cell.read(|s| crate::store::store_snapshot(s));
        snap.fallback_chain = std::sync::Arc::from([std::sync::Arc::from("en")]);
        cell.swap(crate::store::build_store(snap));
        let chain = cell.read(|s| s.fallback_chain.clone());
        assert_eq!(chain.first().map(|s| s.as_ref()), Some("en"));
    }
}
