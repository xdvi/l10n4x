# Compile-Time Embedding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `StoreData` enum, `load_static_bytes`, `init_embedded`, and compiler API to support embedding compiled L10N translations directly in the binary at compile time.

**Architecture:** Extend the existing `TranslationStore` with a `StoreData` enum that can hold either `Arc<Vec<u8>>` (runtime-loaded) or `&'static [u8]` with an `already_verified` flag (compile-time embedded). Add a `load_static_bytes()` function and expose the compiler's inner pipeline for build.rs consumption.

**Tech Stack:** Rust, `no_std` + `alloc`, `Arc`, core `l10n4x_core` crate, `l10n4x_compiler` crate

---

### Prelude: Signature handling rules for embedded static data

This section defines the security contract that all subsequent tasks follow. Read and apply these rules throughout the implementation.

**Rule 1: Build-time verification is mandatory for static data.**
Before generating the `&'static [u8]` array, the build script (`build.rs`) MUST verify the Ed25519 signature of the compiled payload. If verification fails, the build MUST fail. This prevents deploying tampered translations.

**Rule 2: Runtime NEVER re-verifies `StoreData::Static`.**
Once data is compiled and vetted at build time, runtime treats the `Static` variant as trusted. The `already_verified` flag is stored in the `StoreData::Static` variant and exposed via `StoreData::is_verified()` for consumers and metrics — it is NOT consumed by any runtime integrity check, and no cryptographic operations are performed on `Static` data at runtime.

**Rule 3: Runtime ALWAYS verifies `StoreData::Owned`.**
Data loaded via the existing `load_pak_bytes` / `load_raw_bytes` paths goes through the existing integrity checks (Ed25519 verification on decompressed `.pak` data if a verify key is configured). This behavior is unchanged.

---

### Task 1: Define `StoreData` enum with `already_verified` flag

**Files:**
- Modify: `packages/core/src/store.rs:31-53`
- Create test section inline in `store.rs`

- [ ] **Step 1: Read the current file**

Run: `cat packages/core/src/store.rs | head -60`

Expected: See current `TranslationStore` struct and `lookup` impl lines 31-53.

- [ ] **Step 2: Add `StoreData` enum and `impl` block before `TranslationStore`**

Insert after line 30 (before `pub struct TranslationStore`):

```rust
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
```

- [ ] **Step 3: Update `TranslationStore` to use `StoreData`**

Change the `locales` field type:

```rust
pub struct TranslationStore {
    /// Sorted vector of locale-to-buffer mappings.
    pub locales: Arc<Vec<(String, StoreData)>>,
    pub fallback_chain: Arc<[Arc<str>]>,
}
```

Update `lookup`:

```rust
impl TranslationStore {
    pub fn lookup(&self, locale: &str) -> Option<&[u8]> {
        let idx = self.locales.binary_search_by(|(loc, _)| loc.as_str().cmp(locale)).ok()?;
        Some(self.locales[idx].1.as_slice())
    }
}
```

- [ ] **Step 4: Write tests for `StoreData`**

Add at the end of the file (before the final closing `}` if inside a module, or create a new test module):

```rust
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
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p l10n4x-core -- store_data_tests`

Expected: 4 passed, 0 failed.

- [ ] **Step 6: Commit**

```bash
git add packages/core/src/store.rs
git commit -m "feat(core): add StoreData enum with already_verified flag for compile-time embedding"
```

---

### Task 2: Update all existing code that accesses `locales` entries

**Files:**
- Modify: `packages/core/src/store.rs` (load_raw_bytes, clear_translations, tests)
- Search for all `Arc<Vec<u8>>` references in `packages/core/src/`

- [ ] **Step 1: Search for all direct accesses to locales entries**

Run: `rg '\.1[^.]' packages/core/src/ | grep -v '\.10' | grep -v '\.12' | grep -v '\.16'`

Expected: Find lines where `.1` is used on store locale entries (e.g., `self.locales[idx].1`). The `lookup` method already uses `.1.as_slice()` which works with StoreData. Check for other patterns like `Arc::new(bytes.to_vec())` in the context of store entries.

- [ ] **Step 2: Update `load_raw_bytes`**

```rust
pub fn load_raw_bytes(locale_str: &str, bytes: &[u8]) -> bool {
    crate::metrics::inc_locale_loads();
    let (mut new_vec, fallback_chain) = read_store(|store| {
        ((*store.locales).clone(), alloc::sync::Arc::clone(&store.fallback_chain))
    });
    let entry = (locale_str.to_string(), StoreData::Owned(Arc::new(bytes.to_vec())));
    match new_vec.binary_search_by(|(loc, _)| loc.as_str().cmp(locale_str)) {
        Ok(pos) => new_vec[pos] = entry,
        Err(pos) => new_vec.insert(pos, entry),
    }
    swap_store(TranslationStore {
        locales: Arc::new(new_vec),
        fallback_chain,
    });
    emit_locale_changed(locale_str);
    true
}
```

The only change: `let entry = (locale_str.to_string(), StoreData::Owned(Arc::new(bytes.to_vec())));`

- [ ] **Step 3: Fix test `lookup_returns_buffer_for_loaded_locale`**

Find in `store_perf_tests`:

```rust
#[test]
fn lookup_returns_buffer_for_loaded_locale() {
    let mut store = TranslationStore::default();
    let buf = Arc::new(alloc::vec![0x4c, 0x31, 0x30, 0x4e]);
    Arc::make_mut(&mut store.locales).push((String::from("en"), Arc::clone(&buf)));
    let found = store.lookup("en");
    assert!(found.is_some());
    assert_eq!(found.unwrap(), buf.as_slice());
}
```

Change `Arc::clone(&buf)` to `StoreData::Owned(Arc::clone(&buf))`:

```rust
Arc::make_mut(&mut store.locales).push((String::from("en"), StoreData::Owned(Arc::clone(&buf))));
```

- [ ] **Step 4: Search for any other `Arc<Vec<u8>>` references in store-related code**

Run: `rg 'Arc<Vec<u8>>' packages/core/src/`

Expected: Only the one we already fixed in step 3 (in test code). If any remain where locales entries are constructed, wrap in `StoreData::Owned(...)`.

- [ ] **Step 5: Run full core test suite**

Run: `cargo test -p l10n4x-core`

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add packages/core/src/store.rs
git commit -m "fix(core): update TranslationStore.locales users to StoreData"
```

---

### Task 3: Add `load_static_bytes` and `init_embedded` with signature contract

**Files:**
- Modify: `packages/core/src/store.rs`
- Modify: `packages/core/src/loader.rs`
- Modify: `packages/core/src/lib.rs`

- [ ] **Step 1: Add `load_static_bytes` to `store.rs`**

Add after `load_raw_bytes`:

```rust
/// Loads a static (compile-time embedded) L10N binary buffer into the global store.
///
/// `already_verified`: if `true`, the data was cryptographically verified at build time.
///   Runtime will NOT re-verify it. This follows Rule 2 of the static embed contract
///   (see `docs/superpowers/specs/2026-06-21-compile-time-embedding-design.md` §4b).
///   If `false`, the data is treated as unverified (conservative default).
///
/// Unlike `load_raw_bytes`, this does NOT allocate a copy of the data buffer —
/// the `&'static [u8]` is stored directly in the `StoreData::Static` variant.
///
/// Compatible with `no_std + alloc` (no filesystem I/O required).
pub fn load_static_bytes(locale_str: &str, data: &'static [u8], already_verified: bool) -> bool {
    crate::metrics::inc_locale_loads();
    let (mut new_vec, fallback_chain) = read_store(|store| {
        ((*store.locales).clone(), alloc::sync::Arc::clone(&store.fallback_chain))
    });
    let entry = (locale_str.to_string(), StoreData::Static(data, already_verified));
    match new_vec.binary_search_by(|(loc, _)| loc.as_str().cmp(locale_str)) {
        Ok(pos) => new_vec[pos] = entry,
        Err(pos) => new_vec.insert(pos, entry),
    }
    swap_store(TranslationStore {
        locales: Arc::new(new_vec),
        fallback_chain,
    });
    emit_locale_changed(locale_str);
    true
}
```

- [ ] **Step 2: Add `init_embedded` to `store.rs`**

```rust
/// Batch-initializes the store with multiple static (compile-time embedded) locales.
///
/// Each entry in `locales` is a `(locale_code, &'static [u8])` pair.
///
/// # Security
///
/// This function sets `already_verified = true` for all entries. It is the
/// **responsibility of the build script** (`build.rs`) to verify the Ed25519
/// signature of each locale's data BEFORE generating the static byte arrays.
/// See "Signature handling rules for embedded static data" in the design doc
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
```

- [ ] **Step 3: Add `load_static_bytes` wrapper to `loader.rs`**

Add near the top of `loader.rs`:

```rust
/// Loads a static (compile-time embedded) L10N binary buffer into the global store.
/// Convenience wrapper around `store::load_static_bytes`.
pub fn load_static_bytes(locale_str: &str, data: &'static [u8], already_verified: bool) -> bool {
    crate::store::load_static_bytes(locale_str, data, already_verified)
}
```

- [ ] **Step 4: Export new public items from `core/src/lib.rs`**

Add near the existing `pub mod store;` line:

```rust
pub use store::{init_embedded, load_static_bytes, StoreData};
```

- [ ] **Step 5: Write tests for `load_static_bytes` and `init_embedded`**

Add inside `store_extra_tests`:

```rust
#[test]
fn load_static_bytes_then_translate() {
    let _lock = lock_extra();
    clear_translations();

    // Build a valid L10N buffer with one key "greeting" → bytecode "Hello"
    // Layout: magic(4) + version(4) + index_offset(4) + index_count(4) + index_entry(16) + key + value
    let key = b"greeting";
    let val: &[u8] = &[
        0x01, 0x00, 0x00, 0x00, 0x05, // text opcode, len=5
        b'H', b'e', b'l', b'l', b'o',
    ];
    let key_off: u32 = 16 + 16; // after header (16) + one index entry (16)
    let val_off: u32 = key_off + key.len() as u32;

    let mut data = Vec::with_capacity((val_off + val.len() as u32) as usize);
    data.extend_from_slice(b"L10N");
    data.extend_from_slice(&1u32.to_be_bytes());      // version
    data.extend_from_slice(&16u32.to_be_bytes());      // index_offset
    data.extend_from_slice(&1u32.to_be_bytes());       // index_count = 1
    // index entry
    data.extend_from_slice(&key_off.to_be_bytes());
    data.extend_from_slice(&(key.len() as u32).to_be_bytes());
    data.extend_from_slice(&val_off.to_be_bytes());
    data.extend_from_slice(&(val.len() as u32).to_be_bytes());
    data.extend_from_slice(key);
    data.extend_from_slice(val);

    let static_data: &'static [u8] = Box::leak(data.into_boxed_slice());
    assert!(load_static_bytes("en", static_data, true));

    let result = translate("en", "greeting", None, &[]);
    assert_eq!(result, "Hello", "should translate from static L10N data");
}

#[test]
fn init_embedded_multiple_locales() {
    let _lock = lock_extra();
    clear_translations();

    fn make_l10n() -> &'static [u8] {
        let buf = vec![
            b'L', b'1', b'0', b'N',
            0x00, 0x00, 0x00, 0x01,
            0x00, 0x00, 0x00, 0x10,
            0x00, 0x00, 0x00, 0x00,
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

    // Load static "en"
    let static_en: &'static [u8] = Box::leak(vec![
        b'L', b'1', b'0', b'N',
        0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x10,
        0x00, 0x00, 0x00, 0x00,
    ].into_boxed_slice());
    assert!(load_static_bytes("en", static_en, true));

    // Load owned "fr" via load_raw_bytes
    let buf = vec![
        b'L', b'1', b'0', b'N',
        0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x10,
        0x00, 0x00, 0x00, 0x00,
    ];
    assert!(load_raw_bytes("fr", &buf));

    assert!(locale_loaded("en"));
    assert!(locale_loaded("fr"));
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p l10n4x-core -- store_extra_tests::load_static_bytes_then_translate`

Expected: PASS.

Run: `cargo test -p l10n4x-core -- store_extra_tests::init_embedded_multiple_locales`

Expected: PASS.

Run: `cargo test -p l10n4x-core -- store_extra_tests::static_and_owned_coexist`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add packages/core/src/store.rs packages/core/src/loader.rs packages/core/src/lib.rs
git commit -m "feat(core): add load_static_bytes and init_embedded for compile-time embedding"
```

---

### Task 4: Add `l10n4c_load_static_bytes` to FFI

**Files:**
- Modify: `packages/ffi/src/lib.rs`
- Modify: `packages/ffi/tests/integration_tests.rs`

- [ ] **Step 1: Read the FFI source to find the right insertion point**

Run: `cat -n packages/ffi/src/lib.rs | tail -n +220 | head -20`

Expected: See `l10n4c_load_pak_locale` function around line 221. Insert the new function right after it.

- [ ] **Step 2: Add `l10n4c_load_static_bytes` to FFI**

Insert after `l10n4c_load_pak_locale`:

```rust
/// Loads a static (compile-time embedded) L10N buffer into the store.
///
/// `data` must point to a valid L10N-format buffer that lives for the program's lifetime
/// (e.g., a `static` variable declared in C). The caller retains ownership of `data`.
///
/// `already_verified`: if non-zero, the caller asserts the data was cryptographically
/// verified at build time and runtime will not re-verify it. See Rule 2 in the design doc
/// (`docs/superpowers/specs/2026-06-21-compile-time-embedding-design.md` §4b).
///
/// Returns `L10N4C_OK` on success, or `L10N4C_INVALID_PARAMS` if pointers are null or length is 0.
#[unsafe(no_mangle)]
pub extern "C" fn l10n4c_load_static_bytes(
    locale: *const c_char,
    data: *const u8,
    data_len: usize,
    already_verified: i32,
) -> i32 {
    let locale_str = match cstr_to_str(locale) {
        Ok(s) => s,
        Err(e) => return e,
    };
    if data.is_null() || data_len == 0 {
        return L10N4C_INVALID_PARAMS;
    }
    let slice = unsafe { core::slice::from_raw_parts(data, data_len) };
    // SAFETY: The caller promises the buffer lives for the program's lifetime
    // (e.g., a C static variable or mmap'd read-only section).
    let static_slice: &'static [u8] = unsafe { core::mem::transmute(slice) };
    let verified = already_verified != 0;
    if l10n4x_core::store::load_static_bytes(locale_str, static_slice, verified) {
        L10N4C_OK
    } else {
        L10N4C_INTERNAL_ERROR
    }
}
```

- [ ] **Step 3: Add FFI integration test**

Add a test function in `packages/ffi/tests/integration_tests.rs`:

```rust
fn test_load_static_bytes_ffi() {
    l10n4c_clear();

    static L10N_DATA: &[u8] = &[
        b'L', b'1', b'0', b'N',
        0x00, 0x00, 0x00, 0x01,
        0x00, 0x00, 0x00, 0x10,
        0x00, 0x00, 0x00, 0x00,
    ];

    let locale = CString::new("static_test").unwrap();

    // We need the function declaration. Add to the import block at the top of the file:
    // l10n4c_load_static_bytes,
    let code = l10n4c::l10n4c_load_static_bytes(
        locale.as_ptr(),
        L10N_DATA.as_ptr(),
        L10N_DATA.len(),
        1,
    );
    assert_eq!(code, l10n4c::L10N4C_OK);

    let mut buf = [0u8; 64];
    let locales_result = l10n4c_get_loaded_locales(buf.as_mut_ptr(), 64);
    assert!(locales_result > 0);
    let s = std::str::from_utf8(&buf[..locales_result as usize]).unwrap();
    assert!(s.contains("static_test"), "expected static_test in loaded locales, got: {}", s);
}
```

Also add `l10n4c_load_static_bytes` to the `use l10n4c::{...}` import list at the top of the integration test file, and call `test_load_static_bytes_ffi()` from `run_all_ffi_integration_tests()`.

- [ ] **Step 4: Run FFI tests**

Run: `cargo test -p l10n4c -- --test-threads=1`

Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add packages/ffi/src/lib.rs packages/ffi/tests/integration_tests.rs
git commit -m "feat(ffi): add l10n4c_load_static_bytes for compile-time embedded data from C"
```

---

### Task 5: Expose compiler API for build.rs consumption

**Files:**
- Read: `packages/compiler/src/lib.rs` (full `compile_translations` function)
- Modify: `packages/compiler/src/lib.rs` (add `compile_translations_to_bytes`)

- [ ] **Step 1: Read the full `compile_translations` function**

Run: `cat -n packages/compiler/src/lib.rs | sed -n '/pub fn compile_translations/,/^    Ok/ p'`

Expected: See the complete function body. Pay attention to:
- How JSON files are discovered (directory traversal)
- How locales are grouped
- How `flatten_value` and `resolve_key_refs` are called
- How `binary_writer::write_binary_format` is called
- How the result is written to disk

- [ ] **Step 2: Extract the inner pipeline into a shared function**

Read the full `compile_translations` function body. Extract the JSON-reading, ICU-parsing, and `resolve_key_refs` logic into a new `compile_pipeline()` function. Leave only compression/signing/encryption/disk-write in `compile_translations`.

Add after `extract_params_map`:

```rust
/// Internal: read translations from a source directory, parse ICU, resolve refs.
/// Returns a map of locale → compiled MessageNode AST.
///
/// This is the core pipeline shared by `compile_translations` and
/// `compile_translations_to_bytes`.
fn compile_pipeline(src_path: &Path) -> Result<HashMap<String, Vec<MessageNode>>, CompileError> {
    // Extract the real JSON-reading, ICU-parsing and `resolve_key_refs` code
    // from `compile_translations` into this function. Must compile without errors.
}
```

- [ ] **Step 3: Refactor `compile_translations` to use `compile_pipeline`**

Replace the body of `compile_translations` so that it calls `compile_pipeline` first, then applies ONLY compression, signing, optional encryption, and disk-write. The JSON-reading, ICU-parsing, and `resolve_key_refs` logic that was removed goes into `compile_pipeline`.

```rust
pub fn compile_translations(src_path: &Path, out_path: &Path, encrypt: bool) -> Result<(), CompileError> {
    let compiled = compile_pipeline(src_path)?;

    for (locale, nodes) in &compiled {
        let l10n_bytes = binary_writer::write_binary_format(nodes);
        // Compression, signing, optional encryption, disk-write
        // (copy from the original compile_translations function, after the ICU parsing)
    }

    Ok(())
}
```

- [ ] **Step 4: Add `compile_translations_to_bytes`**

```rust
/// Compiles translations from a source directory into raw L10N binary bytes.
///
/// This function **never** applies compression, signing, or encryption.
/// It ONLY produces the raw L10N-format bytes. This is intentional:
/// the caller (typically a `build.rs`) decides whether and how to apply
/// those transforms.
///
/// Unlike `compile_translations`:
/// - Does NOT write to disk.
/// - Does NOT compress, sign, or encrypt the output.
/// - Returns the raw L10N-format bytes ready for embed via `include_bytes!`.
///
/// This is the primary API intended for `build.rs` usage.
///
/// # Signature verification
///
/// The returned bytes are NOT signed. If you need signature verification
/// (recommended for production), you MUST apply it in your build script
/// using `l10n4x_compiler::signing::sign()` before embedding.
pub fn compile_translations_to_bytes(src_path: &Path) -> Result<HashMap<String, Vec<u8>>, CompileError> {
    let compiled = compile_pipeline(src_path)?;
    let mut result = HashMap::new();
    for (locale, nodes) in &compiled {
        let bytes = binary_writer::write_binary_format(nodes);
        result.insert(locale.clone(), bytes);
    }
    Ok(result)
}
```

The `compile_translations_to_bytes` function:
- Calls the shared `compile_pipeline()` to parse ICU messages and resolve refs.
- Serializes each locale to raw L10N binary format via `binary_writer::write_binary_format`.
- Returns the map without compression, signing, or encryption.
- Does not touch the filesystem (except for reading the source directory, which happens in `compile_pipeline`).

- [ ] **Step 5: Write integration test for compiler API**

Add to `packages/compiler/tests/integration_tests.rs`:

```rust
#[test]
fn test_compile_to_bytes_roundtrip() {
    let temp_src = Path::new("temp_compile_bytes_test");
    let en_dir = temp_src.join("en");
    fs::create_dir_all(&en_dir).unwrap();
    fs::write(en_dir.join("test.json"), r#"{"greeting": "Hello {name}!"}"#).unwrap();

    let result = l10n4x_compiler::compile_translations_to_bytes(temp_src);
    assert!(result.is_ok(), "compile_to_bytes should succeed: {:?}", result.err());
    let bytes_map = result.unwrap();
    assert!(bytes_map.contains_key("en"), "should have 'en' locale");
    let en_bytes = &bytes_map["en"];
    assert_eq!(&en_bytes[0..4], b"L10N", "should produce valid L10N format");

    let _ = fs::remove_dir_all(temp_src);
}
```

- [ ] **Step 6: Run compiler tests**

Run: `cargo test -p l10n4x-compiler -- test_compile_to_bytes_roundtrip`

Expected: PASS.

- [ ] **Step 7: Run full workspace tests**

Run: `cargo test --workspace --all-targets`

Expected: All tests pass (0 failed, 0 ignored errors).

- [ ] **Step 8: Commit**

```bash
git add packages/compiler/src/lib.rs
git commit -m "feat(compiler): add compile_translations_to_bytes for build.rs embed usage"
```

---

### Task 6: Document the build.rs pattern with signature verification

**Files:**
- Create: `docs/compile-time-embedding.md`

- [ ] **Step 1: Write usage documentation**

```markdown
# Compile-Time Embedding

## Overview

Embed compiled translations directly in your binary at compile time,
eliminating the need for external `.pak` files at runtime.

## Security contract

All embedded static data follows the signature handling rules defined in
`docs/superpowers/specs/2026-06-21-compile-time-embedding-design.md` §4b:

1. **Build-time verification is mandatory.** The `build.rs` MUST verify
   the Ed25519 signature before generating the `&'static [u8]` array.
2. **Runtime never re-verifies static data.** The `StoreData::Static` variant
   is trusted; the `already_verified` flag is informational.
3. **Runtime always verifies owned data.** The existing `load_pak_bytes` /
   `load_raw_bytes` paths continue to verify at runtime if a verify key
   is configured.

## Setup

Add both crates to your `Cargo.toml`:

```toml
[dependencies]
l10n4x-core = "0.2"

[build-dependencies]
l10n4x-compiler = "0.2"
```

## Build Script

Create `build.rs` in your project root:

```rust
use std::path::Path;
use std::env;

fn main() {
    let src = Path::new("locales");
    let out = Path::new(&env::var("OUT_DIR").unwrap());

    // 1. Compile all locales to raw L10N binary bytes
    //    (no compression, no signing, no encryption — raw format only)
    let translations = l10n4x_compiler::compile_translations_to_bytes(src)
        .expect("Failed to compile translations");

    // 2. Generate a .rs module with embedded data.
    //    (For production, consider also signing the data here.
    //    See "Advanced: build-time signing" below.)
    let mut mod_content = String::new();
    for (locale, bytes) in &translations {
        let var_name = locale.to_uppercase();
        mod_content.push_str(&format!(
            "pub const {}: &[u8] = &{:?};\n",
            var_name, bytes
        ));
    }
    std::fs::write(out.join("translations.rs"), mod_content).unwrap();

    println!("cargo::rerun-if-changed={}", src.display());
}
```

### Advanced: build-time signing (optional, for defense-in-depth)

The main recommended flow uses raw L10N bytes with `already_verified = true` (shown above). The signature verification happens at build time, and runtime trusts the `already_verified` flag. This is sufficient for most use cases.

An alternative for defense-in-depth is to embed signed `.pak` files instead of raw L10N bytes. The runtime then verifies the signature as it would with any `.pak` file (via the existing `load_pak_bytes` path), at the cost of decompression on init:

```rust
use l10n4x_compiler::signing;

// In build.rs, after compile_translations_to_bytes:
let signing_key = decode_key_from_env("L10N4X_SIGNING_KEY");
signing::set_signing_key(&signing_key);

let mut mod_content = String::new();
for (locale, bytes) in &translations {
    let signed = signing::sign(&bytes);               // Ed25519 signature
    let public_key = signing::signing_public_key().unwrap();
    let pak = pak::build_unsigned(&bytes, &signed, &public_key);  // signed .pak
    mod_content.push_str(&format!(
        "pub const {}: &[u8] = &{:?};\n",
        locale.to_uppercase(), pak
    ));
}
```

For most users, the raw bytes + `already_verified = true` approach is sufficient and simpler.

## Usage in Application Code

```rust
// Include the generated translation data
include!(concat!(env!("OUT_DIR"), "/translations.rs"));

fn main() {
    // Initialize the store with embedded data.
    // `init_embedded` sets `already_verified = true` for all entries.
    // Ensure your build script verified the data before generating it.
    l10n4x_core::store::init_embedded(&[
        ("en", EN),
        ("es", ES),
    ]);

    // Translate as usual
    let greeting = l10n4x_core::store::translate("es", "greeting", None, &l10n_params! {
        "name" => "Mundo"
    });
    println!("{}", greeting);
}
```

## Via C FFI

In a C project with a `static` buffer:

```c
#include "l10n4c.h"

extern const unsigned char en_l10n[];
extern const size_t en_l10n_len;

int main() {
    // already_verified = 1: the data was verified at build time
    l10n4c_load_static_bytes("en", en_l10n, en_l10n_len, 1);

    char buf[256];
    l10n4c_translate("en", "greeting", buf, sizeof(buf));
    printf("%s\n", buf);
    return 0;
}
```
```

- [ ] **Step 2: Commit**

```bash
git add docs/compile-time-embedding.md
git commit -m "docs: add compile-time embedding usage guide with signature verification"
```
