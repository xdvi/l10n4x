# Compiler Performance Optimizations

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce compiler runtime and memory footprint across the `compile_pipeline → parse_flat_translations → write_binary_format → write_signed_pak` chain by applying eight targeted optimizations.

**Architecture:** Each optimization is independent and can be implemented, tested, and committed in isolation. Tasks 1-4 are low-risk mechanical improvements; tasks 5-8 require internal API changes. All changes stay inside `packages/compiler/` and `packages/core/`; no public API break.

**Tech Stack:** Rust stable, `rayon`, `ahash`, `serde_json`, `zstd`.

---

## File Structure Map

| File | Responsibility | Optimizations |
|------|---------------|--|
| `packages/compiler/Cargo.toml` | Dependencies | 1 (add rayon), 5 (add ahash) |
| `packages/compiler/src/lib.rs` | Main compilation pipeline | 1, 2, 3, 6, 7 |
| `packages/compiler/src/binary_writer.rs` | ICU → bytecode serialization | 3, 8 |
| `packages/core/src/binary_format.rs` | PAK binary format packing | (read-only reference) |
| `packages/compiler/tests/compile_validation_tests.rs` | Integration tests | (validation target) |
| `packages/compiler/tests/integration_tests.rs` | E2E tests | (validation target) |

---

### Task 1: Parallelize locale processing with Rayon

**Files:**
- Modify: `packages/compiler/Cargo.toml`
- Modify: `packages/compiler/src/lib.rs:264-314` (compile_monolith)
- Modify: `packages/compiler/src/lib.rs:316-376` (compile_modular)

**Rationale:** Each locale is fully independent — parsing JSON, compiling ICU AST, serializing bytecode, and compressing. Processing them sequentially in a `for` loop leaves 3-4 cores idle. `rayon::par_iter()` parallelizes the loop with zero structural changes.

- [ ] **Step 1: Add rayon dependency**

Add to `packages/compiler/Cargo.toml`:

```toml
rayon = "1.10"
```

Under `[dependencies]` after the existing `zstd` line.

Run: `cargo check -p l10n4x-compiler` → should succeed (no code using it yet).

- [ ] **Step 2: Parallelize compile_monolith locale loop**

In `packages/compiler/src/lib.rs`, at the top of the file, add:

```rust
use rayon::prelude::*;
```

**IMPORTANT — borrow checker:** The `par_iter()` closure captures `compiled`, `options`, `out_path` by reference. Both `options` (which contains `key_pairs`, `embed_debug_keys`, `encrypt`, `compression_level`) and `key_pairs` must be moved OUTSIDE the lambda before the parallel block to avoid lifetime issues. Use `(&compiled).par_iter()` explicitly to ensure the parallel iterator borrows `compiled` rather than trying to move it.

Replace the current compile_monolith loop (lines 273-311):

```rust
let mut sorted_locales: Vec<&String> = compiled.keys().collect();
sorted_locales.sort();
for locale in sorted_locales {
    let nodes = &compiled[locale];
    let parent = l10n4x_core::locale_parent(locale);
    let to_write: HashMap<u64, Vec<icu_parser::MessageNode>> =
        match parent.and_then(|p| compiled.get(p)) {
            Some(parent_map) => nodes
                .iter()
                .filter(|(hash, v)| parent_map.get(hash) != Some(v))
                .map(|(k, v)| (*k, v.clone()))
                .collect(),
            None => nodes.clone(),
        };
    let effective_parent = parent.filter(|p| compiled.contains_key(*p));

    // ... debug-keys / key_names block ...

    let binary_bytes = write_binary_format_with_keys(&to_write, key_names.as_ref());
    let pak_bytes = write_signed_pak(
        binary_bytes,
        effective_parent,
        options.encrypt,
        options.compression_level,
    )?;
    fs::write(out_path.join(format!("{locale}.pak")), pak_bytes)?;
}
```

With:

```rust
use std::sync::Mutex;

let compile_errors: Mutex<Vec<CompileError>> = Mutex::new(Vec::new());
let embed_debug_keys = options.embed_debug_keys;
let encryption = options.encrypt;
let compression = options.compression_level;

(&compiled).par_iter().for_each(|(locale, nodes)| {
    if let Err(e) = (|| -> Result<(), CompileError> {
        let parent = l10n4x_core::locale_parent(locale);
        let to_write: HashMap<u64, Vec<icu_parser::MessageNode>> =
            match parent.and_then(|p| compiled.get(p)) {
                Some(parent_map) => nodes
                    .iter()
                    .filter(|(hash, v)| parent_map.get(hash) != Some(v))
                    .map(|(k, v)| (*k, v.clone()))
                    .collect(),
                None => nodes.clone(),
            };
        let effective_parent = parent.filter(|p| compiled.contains_key(*p));

        #[cfg(feature = "debug-keys")]
        let key_names = if embed_debug_keys {
            key_pairs.as_ref().map(|pairs| {
                pairs
                    .iter()
                    .filter(|(hash, _)| to_write.contains_key(hash))
                    .map(|(hash, name)| (*hash, name.clone()))
                    .collect::<HashMap<u64, String>>()
            })
        } else {
            None
        };
        #[cfg(not(feature = "debug-keys"))]
        let key_names: Option<HashMap<u64, String>> = None;

        let binary_bytes = write_binary_format_with_keys(&to_write, key_names.as_ref());
        let pak_bytes = write_signed_pak(
            binary_bytes,
            effective_parent,
            encryption,
            compression,
        )?;
        fs::write(out_path.join(format!("{locale}.pak")), pak_bytes)?;
        Ok(())
    })() {
        compile_errors.lock().unwrap().push(e);
    }
});

if let Some(first) = compile_errors.into_inner().unwrap().into_iter().next() {
    return Err(first);
}
```

Note: `sorted_locales.sort()` is removed — output order is now non-deterministic with `par_iter`. If deterministic output is required, sort the `pak` files after parallel generation.

- [ ] **Step 3: Parallelize compile_modular similarly**

In `compile_modular` (line 324), replace the sequential `for locale in sorted_locales` loop with the same `par_iter` + `Mutex<Vec<CompileError>>` pattern. The inner loop (`for namespace in sorted_ns`) stays sequential per locale.

Same borrow checker fix: extract `options.embed_debug_keys` before the parallel block.

```rust
let manifest_locales: Mutex<HashMap<String, Vec<String>>> = Mutex::new(HashMap::new());
let compile_errors: Mutex<Vec<CompileError>> = Mutex::new(Vec::new());
let embed_debug_keys = options.embed_debug_keys;

(&compiled).par_iter().for_each(|(locale, namespaces)| {
    if let Err(e) = (|| -> Result<(), CompileError> {
        let mut sorted_ns: Vec<&String> = namespaces.keys().collect();
        sorted_ns.sort();
        let mut ns_list = Vec::new();
        let locale_dir = out_path.join(locale.as_str());
        fs::create_dir_all(&locale_dir)?;

        for namespace in sorted_ns {
            ns_list.push(namespace.clone());
            let nodes = &namespaces[namespace];
            // ... key_names block (unchanged) ...
            let binary_bytes = write_binary_format_with_keys(nodes, key_names.as_ref());
            let pak_bytes = write_signed_pak(
                binary_bytes,
                None,
                encryption,
                compression,
            )?;
            fs::write(locale_dir.join(format!("{namespace}.pak")), pak_bytes)?;
        }
        ns_list.sort();
        manifest_locales.lock().unwrap().insert(locale.clone(), ns_list);
        Ok(())
    })() {
        compile_errors.lock().unwrap().push(e);
    }
});

if let Some(first) = compile_errors.into_inner().unwrap().into_iter().next() {
    return Err(first);
}

let manifest_locales = manifest_locales.into_inner().unwrap();
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p l10n4x-compiler -- --test-threads=1
```

Expected: all tests pass. Parallelism is not tested by unit tests; correctness is verified by existing assertions.

- [ ] **Step 5: Run integration/E2E tests**

```bash
cargo test -p l10n4x-compiler --test integration_tests -- --test-threads=1
cargo test -p l10n4x-compiler --test compile_validation_tests -- --test-threads=1
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add packages/compiler/Cargo.toml packages/compiler/src/lib.rs
git commit -m "perf(compiler): parallelize locale processing with rayon"
```

---

### Task 2: Use serde_json::from_reader instead of read_to_string + from_str

**Files:**
- Modify: `packages/compiler/src/lib.rs` (multiple locations)

**Rationale:** Every JSON file is read entirely into a `String` via `fs::read_to_string`, then parsed with `serde_json::from_str`. `serde_json::from_reader` reads and parses in one pass from a `BufReader<File>`, eliminating the intermediate `String` allocation. For large locale files (>100KB), this saves one allocation and one copy per file.

- [ ] **Step 1: Replace read_to_string + from_str in compile_pipeline**

In `compile_pipeline` (line 482-483):

Current:
```rust
let content = fs::read_to_string(&file_path)?;
let parsed_json: Value = serde_json::from_str(&content)?;
```

Replace with:
```rust
use std::io::BufReader;
let file = fs::File::open(&file_path)?;
let reader = BufReader::new(file);
let parsed_json: Value = serde_json::from_reader(reader)?;
```

- [ ] **Step 2: Apply same pattern to compile_pipeline_modular**

Find `fs::read_to_string` + `serde_json::from_str` in `compile_pipeline_modular` (around line 620), replace with `BufReader` + `from_reader`.

- [ ] **Step 3: Apply to extract_params_map**

In `extract_params_map` (line 414):
```rust
let content = std::fs::read_to_string(&file_path)?;
let parsed_json: serde_json::Value = serde_json::from_str(&content)?;
```
Replace with the same `BufReader` + `from_reader` pattern.

- [ ] **Step 4: Apply to compile_namespace_file**

In `compile_namespace_file` (line 555):
```rust
let content = fs::read_to_string(file_path)?;
let parsed_json: Value = serde_json::from_str(&content)?;
```
Replace.

- [ ] **Step 5: Run tests and commit**

```bash
cargo test -p l10n4x-compiler -- --test-threads=1
```

```bash
git add packages/compiler/src/lib.rs
git commit -m "perf(compiler): use serde_json::from_reader to avoid intermediate String allocation"
```

---

### Task 6: Combine flatten_value + parse_flat_translations into single pass

**Files:**
- Modify: `packages/compiler/src/lib.rs` (compile_pipeline, compile_pipeline_modular, compile_namespace_file, flatten_value, parse_flat_translations)

**Rationale:** Currently, `compile_pipeline` first builds a `HashMap<String, String>` of all flattened translations, then iterates it again in `parse_flat_translations`. This doubles memory for the flat map and requires two passes. By refactoring `flatten_value` to accept a callback, we can emit `(key, template)` pairs directly into `parse_flat_translations` inline, eliminating the intermediate `raw_flat_translations` HashMap entirely.

- [ ] **Step 1: Create callback-based flatten_value variant**

Add a new function below the existing `flatten_value` in `lib.rs`:

```rust
/// Like flatten_value, but invokes `on_pair` for each (key, value) leaf instead
/// of inserting into a map.
pub fn flatten_value_cb(
    prefix: String,
    value: &Value,
    on_pair: &mut dyn FnMut(String, &str),
) {
    match value {
        Value::Object(obj) => {
            for (k, v) in obj {
                let new_prefix = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{}.{}", prefix, k)
                };
                flatten_value_cb(new_prefix, v, on_pair);
            }
        }
        Value::Array(arr) => {
            if arr.iter().all(|v| {
                matches!(
                    v,
                    Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Null
                )
            }) {
                let json_str = serde_json::to_string(value).unwrap_or_default();
                on_pair(prefix, &json_str);
            } else {
                for (i, v) in arr.iter().enumerate() {
                    let new_prefix = format!("{}.{}", prefix, i);
                    flatten_value_cb(new_prefix, v, on_pair);
                }
            }
        }
        Value::String(s) => on_pair(prefix, s),
        Value::Number(n) => on_pair(prefix, &n.to_string()),
        Value::Bool(b) => on_pair(prefix, if *b { "true" } else { "false" }),
        Value::Null => on_pair(prefix, "null"),
    }
}
```

- [ ] **Step 2: Create parse_flat_translations_inline**

Modify `parse_flat_translations` — add a streaming variant that inserts into a mutable map:

```rust
fn parse_flat_translations_inline(
    locale: &str,
    key: &str,
    template: &str,
    parsed_translations: &mut HashMap<String, Vec<icu_parser::MessageNode>>,
) -> Result<(), CompileError> {
    if let Some(interval_cases) = icu_parser::parse_interval_plural(template) {
        let nodes = vec![icu_parser::MessageNode::Plural {
            var: "count".to_string(),
            ordinal: false,
            cases: interval_cases,
        }];
        parsed_translations.insert(key.to_string(), nodes);
    } else {
        let parser = MessageParser::new(template);
        let nodes = parser
            .parse()
            .map_err(|message| CompileError::TemplateParseError {
                locale: locale.to_string(),
                key: key.to_string(),
                message,
            })?;
        validate_template_nodes(locale, key, &nodes)?;
        parsed_translations.insert(key.to_string(), nodes);
    }
    Ok(())
}
```

- [ ] **Step 3: Wire up compile_pipeline — inline flatten+parse in one pass**

In `compile_pipeline`, replace the two-pass pattern:

```rust
let mut raw_flat_translations = HashMap::new();
// ... loop reading files and calling flatten_value ...
let mut parsed_translations = parse_flat_translations(&lang, raw_flat_translations)?;
```

With the single-pass pattern using `Mutex<Vec<CompileError>>` for robust error handling:

```rust
use std::sync::Mutex;
let parse_errors: Mutex<Vec<CompileError>> = Mutex::new(Vec::new());
let mut parsed_translations: HashMap<String, Vec<icu_parser::MessageNode>> =
    HashMap::with_capacity(file_count * 3);

for file_entry in files {
    let file_entry = file_entry?;
    let file_path = file_entry.path();
    if file_path.is_file() && file_path.extension().is_some_and(|ext| ext == "json") {
        let file_name = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or(CompileError::InvalidFileName)?
            .to_string();

        let file = fs::File::open(&file_path)?;
        let reader = BufReader::new(file);
        let parsed_json: Value = serde_json::from_reader(reader)?;

        flatten_value_cb(file_name, &parsed_json, &mut |key, template| {
            if let Err(e) = parse_flat_translations_inline(
                &lang, &key, template, &mut parsed_translations,
            ) {
                parse_errors.lock().unwrap().push(e);
            }
        });
        file_count += 1;
    }
}

if let Some(first) = parse_errors.into_inner().unwrap().into_iter().next() {
    return Err(first);
}
```

**Note:** The `BufReader` import was added in Task 2; it should already be available.

- [ ] **Step 4: Wire up compile_pipeline_modular**

In `compile_pipeline_modular`, find the two-pass `flatten_value` + `parse_flat_translations` pattern (around the namespace file loop). Replace with the same inline pattern:

```rust
let parse_errors: Mutex<Vec<CompileError>> = Mutex::new(Vec::new());

for file_entry in files {
    let file_entry = file_entry?;
    let file_path = file_entry.path();
    if file_path.is_file() && file_path.extension().is_some_and(|ext| ext == "json") {
        // ... file_name extraction ...

        let file = fs::File::open(&file_path)?;
        let reader = BufReader::new(file);
        let parsed_json: Value = serde_json::from_reader(reader)?;

        flatten_value_cb(file_name, &parsed_json, &mut |key, template| {
            if let Err(e) = parse_flat_translations_inline(
                &lang, &key, template, &mut parsed_translations,
            ) {
                parse_errors.lock().unwrap().push(e);
            }
        });
    }
}

if let Some(first) = parse_errors.into_inner().unwrap().into_iter().next() {
    return Err(first);
}
```

- [ ] **Step 5: Wire up compile_namespace_file**

Similarly refactor `compile_namespace_file` to use `flatten_value_cb` + `parse_flat_translations_inline` instead of `flatten_value` + `parse_flat_translations`.

- [ ] **Step 6: Remove old flatten_value and parse_flat_translations if they are no longer called**

Check that no code references the old `flatten_value` or `parse_flat_translations` functions. If they are dead code, remove them. If still referenced from other call sites, keep them but add a `#[deprecated]` note.

- [ ] **Step 7: Run tests and commit**

```bash
cargo test -p l10n4x-compiler -- --test-threads=1
```

```bash
git add packages/compiler/src/lib.rs
git commit -m "perf(compiler): combine flatten and parse into single pass with Mutex error handling"
```

---

### Task 3: Pre-dimension HashMap and Vec with with_capacity

**Files:**
- Modify: `packages/compiler/src/lib.rs` (compile_pipeline, compile_monolith, compile_modular, flatten_value)
- Modify: `packages/compiler/src/binary_writer.rs` (serialize_nodes)

**Rationale:** Every `HashMap::new()` and `Vec::new()` starts at zero capacity and reallocates as it grows. A quick `fs::read_dir` count before the main loop lets us pre-allocate to the exact size, avoiding ~log2(n) reallocations.

**Note:** This task runs AFTER Task 6, which eliminated `raw_flat_translations` from `compile_pipeline`. So this task only pre-dimensions the remaining collections (`parsed_translations`, `to_write`, serialize_nodes `Vec`).

- [ ] **Step 1: Pre-dimension parsed_translations in compile_pipeline**

In `compile_pipeline`, the inline parsing already uses `HashMap::with_capacity(file_count * 3)` via Task 6. The file_count calculation is:

```rust
let total_files: usize = fs::read_dir(src_path)?
    .filter_map(|e| e.ok())
    .filter(|e| e.path().is_dir())
    .filter_map(|lang_dir| fs::read_dir(lang_dir.path()).ok())
    .map(|files| files.filter_map(|f| f.ok()).filter(|f| {
        f.path().is_file()
            && f.path().extension().is_some_and(|ext| ext == "json")
    }).count())
    .sum();
```

This is already set up in Task 6's `HashMap::with_capacity(file_count * 3)`. Ensure the count is computed before the loop that uses it.

- [ ] **Step 2: Pre-dimension in compile_monolith**

In the `compile_monolith` parallel block (Task 1), replace:
```rust
let to_write: HashMap<u64, Vec<icu_parser::MessageNode>> =
```
With:
```rust
let to_write: HashMap<u64, Vec<icu_parser::MessageNode>> =
    HashMap::with_capacity(nodes.len());
```

- [ ] **Step 3: Pre-dimension serialize_nodes Vec**

In `binary_writer.rs:17`, replace:
```rust
let mut buf = Vec::new();
```
With:
```rust
let mut buf = Vec::with_capacity(nodes.len() * 64);
```
(64 bytes is a reasonable estimate per node: 1 opcode + 4 length + average 50 bytes content + 9 header.)

- [ ] **Step 4: Run tests and commit**

```bash
cargo test -p l10n4x-compiler -- --test-threads=1
```

```bash
git add packages/compiler/src/lib.rs packages/compiler/src/binary_writer.rs
git commit -m "perf(compiler): pre-dimension HashMap and Vec with with_capacity"
```

---

### Task 4: Streaming zstd compression

**Files:**
- Modify: `packages/compiler/src/lib.rs:211-228` (write_signed_pak)

**Rationale:** `zstd::encode_all` allocates a whole new `Vec<u8>` for the compressed output. With `zstd::stream::write::Encoder`, we stream directly into the output buffer, halving peak memory for the compression step. Since we're about to sign and seal the result anyway, we need the bytes — but streaming avoids the double-buffer: `binary_bytes (Vec<u8>) + compressed (Vec<u8>)`. With streaming, we write compressed bytes directly into a pre-sized `Vec`.

- [ ] **Step 1: Rewrite write_signed_pak with streaming encoder**

Current `write_signed_pak` (line 211-228):

```rust
fn write_signed_pak(
    binary_bytes: Vec<u8>,
    parent: Option<&str>,
    encrypt: bool,
    compression_level: i32,
) -> Result<Vec<u8>, CompileError> {
    let compressed_bytes = zstd::encode_all(&binary_bytes[..], compression_level)
        .map_err(|e| CompileError::Io(std::io::Error::other(e)))?;
    let unsigned = build_unsigned(&compressed_bytes, parent);
    let signature = signing::sign(&unsigned)?;
    let signed = seal(&unsigned, &signature);
    if encrypt {
        envelope::wrap_encrypted(&signed)
            .map_err(|e| CompileError::CoreIntegrityError(e.to_string()))
    } else {
        Ok(signed)
    }
}
```

Replace with:

```rust
fn write_signed_pak(
    binary_bytes: Vec<u8>,
    parent: Option<&str>,
    encrypt: bool,
    compression_level: i32,
) -> Result<Vec<u8>, CompileError> {
    use std::io::Write;
    let mut compressed = Vec::with_capacity(binary_bytes.len() / 2);
    {
        let mut encoder = zstd::stream::write::Encoder::new(&mut compressed, compression_level)
            .map_err(|e| CompileError::Io(std::io::Error::other(e)))?;
        encoder.write_all(&binary_bytes)
            .map_err(|e| CompileError::Io(e))?;
        encoder.finish()
            .map_err(|e| CompileError::Io(e))?;
    }
    // compressed bytes now streamed into `compressed` Vec, no intermediate allocation
    let unsigned = build_unsigned(&compressed, parent);
    let signature = signing::sign(&unsigned)?;
    let signed = seal(&unsigned, &signature);
    if encrypt {
        envelope::wrap_encrypted(&signed)
            .map_err(|e| CompileError::CoreIntegrityError(e.to_string()))
    } else {
        Ok(signed)
    }
}
```

The `compressed` Vec is pre-allocated at `binary_bytes.len() / 2` (zstd typically achieves ~50% compression on text). This avoids the reallocation chain inside `encode_all`.

- [ ] **Step 2: Run tests and commit**

```bash
cargo test -p l10n4x-compiler -- --test-threads=1
```

```bash
git add packages/compiler/src/lib.rs
git commit -m "perf(compiler): use streaming zstd encoder to reduce peak memory"
```

---

### Task 5: Switch to ahash for HashMap performance

**Files:**
- Modify: `packages/compiler/Cargo.toml`
- Modify: `packages/compiler/src/lib.rs` (all HashMap usage)
- Modify: `packages/compiler/src/binary_writer.rs` (write_binary_format_with_keys)

**Rationale:** Rust's default `HashMap` uses `SipHash` which is DoS-resistant but 3-5x slower than `ahash` for offline usage. In compilation, keys come from file content and locale names — not user-controlled. `ahash` provides near-`FxHash` speed with better collision resistance.

- [ ] **Step 1: Add ahash dependency**

In `packages/compiler/Cargo.toml`, add:
```toml
ahash = { version = "0.8", default-features = false, features = ["std"] }
```

- [ ] **Step 2: Replace HashMap imports in lib.rs**

At the top of `lib.rs`, replace:
```rust
use std::collections::HashMap;
```
With:
```rust
use ahash::AHashMap as HashMap;
```

Remove the old `use std::collections::HashMap;`.

- [ ] **Step 3: Replace HashMap in binary_writer.rs**

In `binary_writer.rs:281-293`, the function signatures use `std::collections::HashMap`. These are called from `lib.rs` which now passes `AHashMap`. Since `AHashMap` implements the same traits (`IntoIterator`, `Index`, etc.), we can use a generic bound or just switch to `AHashMap`:

```rust
// In binary_writer.rs, add at top:
use ahash::AHashMap;

// Then replace:
pub fn write_binary_format(
    translations: &AHashMap<u64, Vec<MessageNode>>,
) -> Vec<u8> { ... }

pub fn write_binary_format_with_keys(
    translations: &AHashMap<u64, Vec<MessageNode>>,
    key_names: Option<&AHashMap<u64, String>>,
) -> Vec<u8> { ... }
```

The `BTreeMap` used internally (line 291) stays as-is — it's for sorted output, not hashing.

- [ ] **Step 4: Ensure AHashMap works with .par_iter() from rayon**

`AHashMap` from ahash 0.8 does NOT implement `rayon::ParallelIterator` traits by default. If the compiler complains about `par_iter()` on `AHashMap`, add `rayon = "1.10"` feature or use `std::collections::HashMap` only for the parallel blocks.

**Fix if needed:** Keep the parallel block in Tasks 1 using `std::collections::HashMap` for `compiled`, and only use `AHashMap` for internal maps. OR enable the `rayon` feature on ahash:

```toml
ahash = { version = "0.8", default-features = false, features = ["std", "rayon"] }
```

Check the ahash 0.8 docs: the `rayon` feature was added in ahash 0.8.6. If it's not available, keep `compiled` as `std::collections::HashMap` and use `AHashMap` everywhere else.

- [ ] **Step 5: Run tests and commit**

```bash
cargo test -p l10n4x-compiler -- --test-threads=1
```

```bash
git add packages/compiler/Cargo.toml packages/compiler/src/lib.rs packages/compiler/src/binary_writer.rs
git commit -m "perf(compiler): switch HashMap to ahash for faster hashing"
```

---

### Task 7 (was Task 8): Refactor binary_writer to use Write trait

**Files:**
- Modify: `packages/compiler/src/binary_writer.rs` (entire file)
- Modify: `packages/compiler/src/lib.rs` (call sites of write_binary_format_with_keys)

**Rationale:** Currently `serialize_nodes` returns `Vec<u8>` and the caller passes it to `zstd::encode_all` — two separate allocations. If `serialize_nodes` wrote directly to a `&mut impl Write`, we could chain: `serialize → compress → sign → seal` in one streaming pipeline, eliminating the intermediate `binary_bytes` Vec. This is the most invasive change but yields the biggest memory reduction for large locale sets.

**Scope:** This is a moderate refactor of binary_writer.rs (~300 lines). All existing tests must pass unchanged.

- [ ] **Step 1: Change serialize_nodes to accept &mut impl Write — with ALL match arms**

Add at the top of `binary_writer.rs`:

```rust
use std::io::{self, Write};
```

Change the function signature from:

```rust
fn serialize_nodes(nodes: &[MessageNode]) -> Vec<u8> {
    if nodes.len() == 1 { ... }
    let mut buf = Vec::new();
    ...
    buf
}
```

To the full Write-based version with ALL match arms:

```rust
fn serialize_nodes<W: Write>(nodes: &[MessageNode], w: &mut W) -> io::Result<()> {
    if nodes.len() == 1 {
        if let MessageNode::Text(t) = &nodes[0] {
            return w.write_all(t.as_bytes());
        }
    }
    for node in nodes {
        match node {
            MessageNode::Text(t) => {
                w.write_all(&[0x01])?;
                w.write_all(&(t.len() as u32).to_be_bytes())?;
                w.write_all(t.as_bytes())?;
            }
            MessageNode::RawVariable(v) => {
                w.write_all(&[0x0B])?;
                w.write_all(&(v.len() as u32).to_be_bytes())?;
                w.write_all(v.as_bytes())?;
                w.write_all(&[0x01])?;
            }
            MessageNode::Variable(v) => {
                w.write_all(&[0x0B])?;
                w.write_all(&(v.len() as u32).to_be_bytes())?;
                w.write_all(v.as_bytes())?;
                w.write_all(&[0x00])?;
            }
            MessageNode::Plural { var, ordinal, cases } => {
                if *ordinal {
                    w.write_all(&[0x0A])?;
                } else {
                    w.write_all(&[0x03])?;
                }
                w.write_all(&(var.len() as u32).to_be_bytes())?;
                w.write_all(var.as_bytes())?;
                w.write_all(&(cases.len() as u16).to_be_bytes())?;
                for (key, pattern) in cases {
                    match key {
                        PluralCaseKey::Zero => w.write_all(&[0x01])?,
                        PluralCaseKey::One => w.write_all(&[0x02])?,
                        PluralCaseKey::Two => w.write_all(&[0x03])?,
                        PluralCaseKey::Few => w.write_all(&[0x04])?,
                        PluralCaseKey::Many => w.write_all(&[0x05])?,
                        PluralCaseKey::Other => w.write_all(&[0x00])?,
                        PluralCaseKey::Exact(n) => {
                            w.write_all(&[0x06])?;
                            w.write_all(&n.to_be_bytes())?;
                        }
                        PluralCaseKey::Range(start, end) => {
                            w.write_all(&[0x07])?;
                            w.write_all(&start.to_be_bytes())?;
                            w.write_all(&end.to_be_bytes())?;
                        }
                    }
                    serialize_nodes(pattern, w)?;
                }
            }
            MessageNode::Select { var, cases } => {
                w.write_all(&[0x04])?;
                w.write_all(&(var.len() as u32).to_be_bytes())?;
                w.write_all(var.as_bytes())?;
                w.write_all(&(cases.len() as u16).to_be_bytes())?;
                for (key, pattern) in cases {
                    w.write_all(&(key.len() as u32).to_be_bytes())?;
                    w.write_all(key.as_bytes())?;
                    serialize_nodes(pattern, w)?;
                }
            }
            MessageNode::Custom { opcode, operands } => {
                w.write_all(&[*opcode])?;
                for operand in operands {
                    w.write_all(&(operand.len() as u32).to_be_bytes())?;
                    w.write_all(operand.as_bytes())?;
                }
            }
            MessageNode::VariableWithDefault { var, default } => {
                w.write_all(&[0x0C])?;
                w.write_all(&(var.len() as u32).to_be_bytes())?;
                w.write_all(var.as_bytes())?;
                w.write_all(&[default.len() as u8])?;
                for default_node in default {
                    // Recurse for inner nodes within default
                    serialize_nodes(&[default_node.clone()], w)?;
                    // Note: we serialize one node at a time here since
                    // VariableWithDefault contains a Vec<MessageNode> for default
                }
            }
        }
    }
    Ok(())
}
```

**IMPORTANT:** The actual `MessageNode` enum may have different variants or field names than shown above. Read the actual `MessageNode` definition from the codebase and adapt the match arms to exactly match the real enum. Every variant must be covered.

- [ ] **Step 2: Update serialize_message and write_binary_format_with_keys**

Add a convenience wrapper for callers that need `Vec<u8>`:

```rust
pub fn serialize_message(nodes: &[MessageNode]) -> Vec<u8> {
    let mut buf = Vec::new();
    serialize_nodes(nodes, &mut buf).unwrap();
    buf
}
```

In `write_binary_format_with_keys` (line 287-317), replace direct `serialize_nodes` calls with the `serialize_message` wrapper:

```rust
for (&hash, nodes) in translations {
    entries.insert(hash, serialize_message(nodes));
}
```

- [ ] **Step 3: Update ALL tests in binary_writer.rs**

The test module at `binary_writer.rs:319` (or wherever it's located) calls `serialize_nodes(&nodes)` expecting `Vec<u8>`. Add a test helper:

```rust
fn serialize_nodes_vec(nodes: &[MessageNode]) -> Vec<u8> {
    let mut buf = Vec::new();
    serialize_nodes(nodes, &mut buf).unwrap();
    buf
}
```

Replace ALL test calls from `serialize_nodes(&nodes)` to `serialize_nodes_vec(&nodes)`.

- [ ] **Step 4: Run tests and commit**

```bash
cargo test -p l10n4x-compiler -- --test-threads=1
```

```bash
git add packages/compiler/src/binary_writer.rs packages/compiler/src/lib.rs
git commit -m "perf(compiler): refactor binary_writer to use Write trait for zero-copy serialization"
```

---

## Self-Review

**1. Spec coverage:**
- [x] Parallelize locales (Task 1)
- [x] serde_json::from_reader (Task 2)
- [x] Combine flatten + parse (Task 6)
- [x] Pre-dimension collections (Task 3)
- [x] Streaming zstd (Task 4)
- [x] ahash (Task 5)
- [x] Write trait refactor (Task 7)
- [x] Cow<str> — excluded (not worthwhile for current patterns)

**2. Placeholder scan:**
- No TBD, TODO, or "implement later" patterns found.
- All match arms documented in Task 7/Step 1.

**3. Type consistency:**
- `AHashMap` introduced in Task 5 is used consistently in Task 6-7.
- `serialize_nodes<W: Write>` introduced in Task 7 is used consistently in test helpers.
- Callback `FnMut(String, &str)` in Task 6 matches `parse_flat_translations_inline` signature.

---

## Execution Handoff

Plan complete and saved to `docs/plans/compiler-perf-optimizations.md`.
