//! Registry of scoped [`StoreCell`] instances keyed by [`StoreHandle`].

use crate::error::{CoreError, CoreResult};
use crate::store_cell::StoreCell;
use core::num::NonZeroU32;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Opaque handle identifying a scoped translation store (not the process-global cell).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StoreHandle(NonZeroU32);

impl StoreHandle {
    /// Sentinel for APIs that accept `Option<StoreHandle>` — `None` means the global cell.
    pub const GLOBAL: Option<StoreHandle> = None;

    /// Constructs a handle from a raw non-zero id (e.g. FFI interop).
    pub fn from_raw(id: u32) -> CoreResult<Self> {
        NonZeroU32::new(id)
            .map(StoreHandle)
            .ok_or(CoreError::InvalidStoreHandle(id))
    }

    /// Returns the raw handle id.
    pub fn raw(self) -> u32 {
        self.0.get()
    }
}

fn registry() -> &'static Mutex<HashMap<u32, StoreCell>> {
    static REG: OnceLock<Mutex<HashMap<u32, StoreCell>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

static NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// Creates a new isolated translation store and returns its handle.
pub fn create_store() -> CoreResult<StoreHandle> {
    let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let handle = StoreHandle::from_raw(id)?;
    let cell = StoreCell::new(crate::store::TranslationStore::default());
    registry()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .insert(id, cell);
    Ok(handle)
}

/// Destroys a scoped store, releasing its cell from the registry.
pub fn destroy_store(handle: StoreHandle) -> CoreResult<()> {
    let mut reg = registry().lock().unwrap_or_else(|p| p.into_inner());
    reg.remove(&handle.raw())
        .ok_or(CoreError::InvalidStoreHandle(handle.raw()))?;
    Ok(())
}

/// Runs `f` with the [`StoreCell`] for `handle` (`None` = global cell).
pub(crate) fn with_cell<R>(
    handle: Option<StoreHandle>,
    f: impl FnOnce(&StoreCell) -> R,
) -> CoreResult<R> {
    match handle {
        None => Ok(f(StoreCell::global())),
        Some(h) => {
            let reg = registry().lock().unwrap_or_else(|p| p.into_inner());
            let cell = reg
                .get(&h.raw())
                .ok_or(CoreError::InvalidStoreHandle(h.raw()))?;
            Ok(f(cell))
        }
    }
}
