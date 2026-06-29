//! Safe deferred/immediate memory reclamation helpers.

#[cfg(feature = "std")]
struct SendPtr<T>(*mut T);

#[cfg(feature = "std")]
// SAFETY: `SendPtr` wraps a raw pointer that is only accessed inside the deferred closure
// passed to `guard.defer_unchecked`, which runs after the current epoch ends and only once.
// The `*mut T` is obtained from `Box::into_raw`, and the closure is the exclusive owner.
unsafe impl<T> Send for SendPtr<T> {}

/// # Safety
///
/// Defer-deletes the boxed pointer using epoch-based reclamation.
/// The caller must guarantee that:
/// 1. `ptr` points to a valid heap allocation created via `Box::into_raw`.
/// 2. No other thread will dereference `ptr` after the current epoch ends.
#[cfg(feature = "std")]
pub(crate) fn schedule_drop<T>(ptr: *mut T) {
    let send_ptr = SendPtr(ptr);
    let guard = crossbeam_epoch::pin();
    unsafe {
        // SAFETY: The closure is deferred until the current epoch is clean.
        // `SendPtr` safely wraps the raw pointer to satisfy the `Send` bound on `defer_unchecked`.
        guard.defer_unchecked(move || {
            let _ = alloc::boxed::Box::from_raw(send_ptr.0);
        });
    }
}

/// # Safety
///
/// Immediately drops/deletes the boxed pointer.
/// In single-threaded (`no_std`) contexts, there are no concurrent readers,
/// so it is safe to reclaim the memory immediately.
/// The caller must guarantee that:
/// 1. `ptr` points to a valid heap allocation created via `Box::into_raw`.
/// 2. No other reference to `ptr` exists at the time of calling.
#[cfg(not(feature = "std"))]
pub(crate) fn schedule_drop<T>(ptr: *mut T) {
    unsafe {
        // SAFETY: Reclaims the pointer allocated via Box, immediately dropping the boxed value.
        let _ = alloc::boxed::Box::from_raw(ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;

    #[test]
    fn schedule_drop_box() {
        let ptr = Box::into_raw(Box::new(42u32));
        schedule_drop(ptr);
    }
}
