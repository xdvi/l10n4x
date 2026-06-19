//! `l10n4x-core` is the runtime component of the `l10n4x` localization workspace.
//! It supports high-performance, `#![no_std]` compatible, zero-allocation
//! formatting of ICU MessageFormat-style localization strings directly from
//! decrypted `.pak` files in memory.

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod binary_format;
pub mod crypto;
pub mod formatter;
pub mod loader;
pub mod store;

#[cfg(test)]
mod tests;
