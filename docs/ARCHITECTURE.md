# Architecture

l10n4x is a localization toolkit built around a compile-time pipeline: JSON translation files are compiled into signed, compressed binary `.pak` files that a minimal `no_std` runtime can load and format. All heavy work (parsing, compilation, type generation) happens offline.

---

## Data Flow

```
  JSON locale files (per locale, per namespace)
         |
         v
  [l10n4x-compiler]
     |-- flatten_value() -- dot-notation flat keys
     |-- icu_parser -- ICUMF2 AST (MessageNode)
     |-- resolve_key_refs -- $t(key) cross-references
     |-- binary_writer -- serialize to L10N opcode bytecode
     |-- zstd -- zstd compression
     |-- Ed25519 signing -- mandatory
     |-- optional AES-GCM envelope (L10E)
         |
         v
  .pak file (per locale, binary, signed)
         |
         v
  [l10n4x-core runtime]
     |-- loader -- decompress + verify signature
     |-- store -- RCU-protected TranslationStore
     |-- binary_format -- O(log N) binary search lookup
     |-- formatter -- opcode interpreter (0x01-0x0C)
     |-- plural_rules -- CLDR 120+ locales
     |-- number_format -- locale-aware decimal/percent/currency
     |-- date_format -- ISO 8601 + locale patterns
     |-- reltime -- relative time ("2 hours ago")
     |-- list_format -- "A, B, and C"
         |
         v
  Output string (locale-formatted, interpolated)
```

---

## Packages

### `l10n4x-core` (runtime)

| Aspect | Detail |
|--------|--------|
| Path | `packages/core/` |
| Crate | `l10n4x-core` |
| Target | `no_std` + `alloc` (optional `std`) |
| Modules | 12 modules (see below) |
| Features | `std`, `alloc`, `encryption` |
| Dependencies | `miniz_oxide`, `ed25519-dalek` (optional), `aes-gcm` (optional), `crossbeam-epoch` (optional std) |

Modules:

| Module | Purpose |
|--------|---------|
| `binary_format.rs` | O(log N) binary search reader for `L10N` format |
| `envelope.rs` | `L10E` encrypted outer envelope parsing |
| `encryption.rs` | AES-256-GCM encrypt/decrypt (optional) |
| `formatter.rs` | Opcode interpreter for bytecode `0x01`-`0x0C` |
| `plural_rules.rs` | CLDR plural categories for 120+ locales |
| `number_format.rs` | Locale-aware number/percent/currency formatting |
| `date_format.rs` | ISO 8601 date/time/datetime formatting |
| `reltime.rs` | Relative time strings ("in 3 days", "2 minutes ago") |
| `list_format.rs` | Conjunction/disjunction/unit list formatting |
| `integrity.rs` | Ed25519 signature verification |
| `loader.rs` | `.pak` decompression and store loading |
| `store.rs` | Lock-free RCU translation store; scoped `*_for_store` APIs |
| `store_cell.rs` | Reusable RCU cell (`AtomicPtr` + writer mutex) per store |
| `store_registry.rs` | `StoreHandle` registry for tenant-scoped cells |

### `l10n4x-compiler` (build-time)

| Aspect | Detail |
|--------|--------|
| Path | `packages/compiler/` |
| Crate | `l10n4x-compiler` |
| Dependencies | `serde_json`, `miniz_oxide`, `ed25519-dalek`, `l10n4x-core` |

Modules:

| Module | Purpose |
|--------|---------|
| `icu_parser.rs` | ICUMF2 parser (plural, select, number, date, list, reltime, variables) |
| `binary_writer.rs` | Serializes AST into `L10N` opcode bytecode |
| `signing.rs` | Ed25519 signing (build-time only; never in runtime) |
| `lib.rs` | `flatten_value()`, `compile_translations()`, `extract_params_map()`, `resolve_key_refs()` |

### `l10n4x-toolkit` (CLI)

| Aspect | Detail |
|--------|--------|
| Path | `packages/cli/` |
| Crate | `l10n4x-toolkit` |
| Binary | `l10n4x` |
| Commands | 9 commands |

Commands:

| Command | Description |
|---------|-------------|
| `init` | Interactive wizard to create `l10n4x.config.json` |
| `build` | Compile `.pak` files and generate type-safe bindings |
| `validate` | Check translation key consistency across locales |
| `dev` | Hot-reload dev server (SSE + file watcher) |
| `generate` | Generate bindings for a specific target |
| `check` | CI gate -- verify code keys match locale keys |
| `extract` | Scan source files, add missing keys to locale JSON |
| `pseudo` | Generate pseudolocale for layout/overflow testing |
| `stats` | Translation coverage report |

CLI binding targets (5):

| Target | Path | Output |
|--------|------|--------|
| TypeScript | `targets/typescript.rs` | Thin `generated.ts` — `Keys`, `LocaleKey`, param types only |
| Go | `targets/go.rs` | `generated.go` with typed `T()` function |
| Python | `targets/python.rs` | `generated.py` with `translate()` function |
| C | `targets/c.rs` | `l10n4c.h` + `generated.c` with typed `LOCALE_KEY_*` constants |
| Flutter/Dart | `targets/flutter.rs` | `generated.dart` with typed getters |

Web runtime and framework adapters (React, Vue, Svelte, Angular) live in the separate [`l10n4x-js`](https://github.com/xdvi/l10n4x-js) monorepo: `@l10n4x/wasm`, `@l10n4x/runtime`, `@l10n4x/react`, `@l10n4x/vue`, `@l10n4x/svelte`, `@l10n4x/angular`.

### `l10n4c` (C FFI)

| Aspect | Detail |
|--------|--------|
| Path | `packages/ffi/` |
| Crate | `l10n4c` |
| Library types | `cdylib`, `staticlib`, `rlib` |
| Header | `l10n4c.h` (distributed with binary releases) |

Covers: load paks, translate with typed `L10n4cParam` arrays, manage fallback locale, missing key callbacks.

### `l10n4x-wasm` (WASM bindings)

| Aspect | Detail |
|--------|--------|
| Path | `packages/wasm/` |
| Crate | `l10n4x-wasm` |
| Technology | `wasm-bindgen` + `js-sys` |

Covers: `l10n4x_translate()`, `l10n4x_load_pak_bytes()`, `l10n4x_set_fallback_chain()`, etc.

---

## `.pak` File Format

For full specification, see [PAK_FORMAT.md](./PAK_FORMAT.md).

### Outer container (`L10P`)

```
[L10P magic: 4B][Version: 4B][Payload len: 4B][zstd payload: N B][Ed25519 sig: 64B]
```

### Optional encrypted envelope (`L10E`)

```
[L10E magic: 4B][Version: 4B][Blob len: 4B][AES-256-GCM ciphertext: N B]
```

### Inner binary format (`L10N`)

After decompression and signature verification, the inner format:

```
[Magic "L10N": 4B][Format version: 4B][Index offset: 4B][Index count: 4B]
[Data pool: keys + bytecode values ...]
[Index: 16B per entry (key_offset, key_len, val_offset, val_len)]
```

The index is sorted alphabetically by key, enabling O(log N) binary search.

### Opcodes (0x01-0x0C)

| Opcode | Name | Purpose |
|--------|------|---------|
| `0x01` | Text | Literal UTF-8 string |
| `0x02` | Variable | Simple `{name}` interpolation |
| `0x03` | Plural | Cardinal plural selection |
| `0x04` | Select | String match selection |
| `0x05` | Number | Locale number formatting |
| `0x06` | Date | Date/time/datetime formatting |
| `0x07` | Variable w/ Default | `{name\|Default}` interpolation |
| `0x08` | RelTime | Relative time string |
| `0x09` | List | Conjunction/disjunction/unit list |
| `0x0A` | Ordinal Plural | Ordinal plural selection |
| `0x0B` | Variable w/ HTML flag | `{name}` with escaping control |
| `0x0C` | Variable w/ Default + HTML flag | Combined default + escaping |

---

## Security Model

### Principles

1. **Mandatory signing.** Every `.pak` file is Ed25519-signed. The runtime refuses to load unsigned or tampered paks.
2. **Signing key isolation.** The signing seed (`L10N4X_SIGNING_KEY`) is only accessible to the compiler crate and CLI. The core runtime has no signing capability -- it only verifies.
3. **Optional encryption.** AES-256-GCM (`L10E` envelope) wraps the signed pak for confidentiality in transit. Encryption does not replace signing -- it wraps the already-signed container.
4. **No `eval()`.** The runtime never evaluates dynamic code. All formatting is opcode-based with fixed locale tables.
5. **Input validation.** All external inputs (locale codes, file paths, key names) are validated for length, character set, and path traversal before use.

For full threat model, see [THREAT_MODEL.md](./THREAT_MODEL.md).

### Key Architecture

```
Build time:                    Runtime:
  L10N4X_SIGNING_KEY            verifyPublicKey (hex, embedded)
        |                              |
  [compiler] sign()              [core] verify()
        |                              |
  .pak (signed) ──────────────> .pak (verified)
```

---

## Translation Flow (Runtime)

```
translate("en", "common.welcome", { name: "Diego" })
    |
    v
store.lookup("en")
    |  O(log N) binary search in TranslationStore.locales
    v
BinaryFormatReader::new(buf)
    |  Validate L10N magic + version
    v
reader.lookup("common.welcome")
    |  O(log N) binary search in sorted index
    v
bytecode: [0x01, "Hello ", 0x0B, "name", 0x00]
    |
    v
format_message(bytecode, locale, params, writer)
    |  Interpret opcodes sequentially
    |  0x01 = Text("Hello ")
    |  0x0B = Variable("name", flags=0) → lookup "name" in params → html_escape
    v
"Hello Diego"  (written to output buffer)
```

### Fallback chain

```
translate("es-MX", "key")
    |
    v
1. Try "es-MX" locale
2. BCP-47 subtag parent: try "es"
3. Walk configured fallback chain (e.g. ["en-US", "en"])
4. If all miss: call missing key handler, return raw key
```

---

## Thread Safety

The global `TranslationStore` uses RCU (Read-Copy-Update) for lock-free concurrent access:

- **Readers** (`translate`, `key_exists`, `locale_loaded`): Call `read_store()` which pins an epoch guard via `crossbeam-epoch`, then loads the store pointer atomically. Multiple readers proceed in parallel with no contention.
- **Writers** (`load_raw_bytes`, `load_namespace`, `swap_store`, `clear_translations`): Clone the current snapshot, apply mutations, then publish via `swap_store`. Writers are serialized with `STORE_WRITE_MUTEX` so concurrent reloads cannot tear state; readers still proceed lock-free.
- **Modular bundles** (opt-in): `build` with `"bundles": { "mode": "modular" }` emits `{locale}/{namespace}.pak` plus `namespaces.json`. `load_namespace` merges namespace paks into a locale buffer; `init_modular` preloads namespaces listed in the manifest.
- **Reclamation**: `schedule_drop()` defers the `Box::from_raw` drop until the current epoch ends (under `std` with `crossbeam-epoch`). Under `no_std` (single-threaded), drops happen immediately.

This design provides:
- Wait-free reads (no locks, no spinning)
- No read-side contention (no atomics on the hot path)
- Safe hot-reload (swap store while readers hold references)

### Scoped stores (P2.5)

Server-side runtimes can hold multiple isolated `TranslationStore` instances without mutating the process-global default:

| Component | Role |
|-----------|------|
| `StoreCell` | RCU cell — one per global store or per `StoreHandle` |
| `StoreHandle` | Opaque tenant id (`NonZeroU32`); `None` selects the global cell |
| `store_registry` | `create_store` / `destroy_store`; serializes registry mutations |
| `*_for_store` APIs | `translate_for_store`, `try_load_static_bytes_for_store`, `clear_translations_for_store`, `try_ota_reload_pak_for_store`, etc. |

**Default behavior:** legacy `translate()`, `read_store()`, and `l10n4c_*` global exports delegate to `StoreHandle::GLOBAL` (`None`) — unchanged for existing callers.

**TLS translate cache:** keys include `store_id` (`0` = global) plus locale and key hashes so concurrent tenants cannot pollute each other's fast or full caches.

**FFI:** `l10n4c_store_create`, `l10n4c_store_destroy`, `l10n4c_store_load_pak_locale`, `l10n4c_store_translate`, `l10n4c_store_clear`, `l10n4c_store_ota_reload_pak` — handle `0` is reserved and invalid.

**Out of scope (P2.5.1):** overlay/inheritance (tenant inherits base paks), WASM multi-instance, Angular/React binding changes.

---

## Feature Gates

```
l10n4x-core:
  default = ["std"]
  std     = ["alloc", "encryption", "crossbeam-epoch"]
  alloc   = ["ed25519-dalek"]
  encryption = ["aes-gcm"]

l10n4x-compiler:
  always std

l10n4x-toolkit (CLI):
  always std

l10n4c (FFI):
  always std

l10n4x-wasm:
  wasm target + std
```

The `no_std` configuration (`--no-default-features --features alloc`) removes:
- Crossbeam epoch (RCU becomes immediate swap + drop)
- AES-GCM encryption (L10E envelope rejected)
- File system I/O (`load_pak_directory`, `load_pak_locale`)
- `std::error::Error` impls

---

## Directory Structure

```
l10n4x/
  Cargo.toml              # Workspace root
  l10n4x.config.json      # Project config
  packages/
    core/                 # l10n4x-core (no_std runtime)
      src/                # 12 modules
      tests/              # Integration tests
      benches/            # Benchmarks
    compiler/             # l10n4x-compiler (build-time)
      src/                # icu_parser, binary_writer, signing
      tests/              # Integration tests
    cli/                  # l10n4x-toolkit (CLI binary)
      src/
        targets/          # Target generators (6 + growing)
        templates/        # Template files for bindings
      tests/              # Dev server tests
    ffi/                  # l10n4c (C FFI)
      src/
      tests/
    wasm/                 # l10n4x-wasm (WASM bindings)
      src/
  docs/                   # Documentation
    PAK_FORMAT.md
    THREAT_MODEL.md
    ARCHITECTURE.md
    MIGRATION.md
  locales/                # Source translation JSON files
  examples/               # Framework integration examples
  scripts/                # CI/dev helper scripts
  .github/workflows/      # CI/CD pipelines
```
