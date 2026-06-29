# `.pak` File Format (v1)

All multi-byte integers are **big-endian**.

## Signed container (`L10P`)

Two header formats exist. The parser autodetects which one is used.

### Old format (v1, flags = 0)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `L10P` |
| 4 | 4 | Version `1` |
| 8 | 4 | Payload length (N) |
| 12 | N | zstd-compressed inner `L10N` binary |
| 12+N | 64 | Ed25519 signature over bytes `[0..12+N)` |

### New format (v1, flags bit 0 = has_parent)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `L10P` |
| 4 | 4 | Version `1` |
| 8 | 4 | Flags (bit 0 = has_parent_locale) |
| 12 | 4 | Payload length (N) |
| 16 | 1 | Parent locale length (only if bit 0 set) |
| 17 | * | Parent locale UTF-8 (only if bit 0 set) |
| ... | N | zstd-compressed inner `L10N` binary |
| ...+N | 64 | Ed25519 signature over bytes `[0..parent_end+N)` |

Signature verification is **mandatory** at runtime. Unsigned or tampered paks are rejected.

## Optional encrypted envelope (`L10E`)

When `"encrypt": true` in `l10n4x.config.json`, each signed `L10P` pak is wrapped:

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `L10E` |
| 4 | 4 | Version `1` |
| 8 | 4 | Blob length (N) |
| 12 | N | AES-256-GCM ciphertext (12-byte nonce prepended) |

The AES-GCM plaintext is the complete signed `L10P` pak (including its Ed25519 signature). Encryption is applied **after** signing; decryption happens **before** signature verification.

## Keys

| Key | Where | Purpose |
|-----|-------|---------|
| Signing seed (32 B) | `L10N4X_SIGNING_KEY` env, build only | Signs inner `L10P` paks |
| Public key (32 B) | `verifyPublicKey` in config + client bindings | Verifies signatures at runtime |
| AES key (32 B) | `L10N4X_ENCRYPT_KEY` env (opt-in) | Encrypts/decrypts `L10E` envelope |

The signing seed never ships in client binaries. The AES key is only required when `encrypt` is enabled; it does not replace signature verification.

## Inner Binary Opcodes

Inside the decompressed `L10N` block, the value of each key is a sequence of opcodes.

**Optimization:** When a value is a single text node (no variables, plurals, etc.), it is stored as raw UTF-8 bytes without the `[0x01][len]` prefix. The runtime detects this at format time: any first byte `0x00` or `> 0x0E` is treated as raw text rather than an opcode.

| Opcode | Name | Encoding |
|--------|------|----------|
| `0x01` | Text | `[u32: len][len bytes: text]` |
| `0x02` | Variable | `[u32: var_name_len][var_name_bytes]` |
| `0x03` | Plural | `[u32: var_name_len][var_name_bytes][u16: case_count][cases...]` — each case: `[u8: type][extras][u32: pat_len][pat_bytes]` where type `0x00`=other, `0x01`=exact (`f64`), `0x02`–`0x06`=zero/one/two/few/many, `0x07`=inclusive range (`i32 min`, `i32 max`; `max = i32::MAX` = open-ended) |
| `0x04` | Select | `[u32: var_name_len][var_name_bytes][u16: case_count][cases...]` |
| `0x05` | Number | `[u32: var_name_len][var_name_bytes][u8: style][style extras]` where style: `0x00`=decimal, `0x01`=percent, `0x02`=integer, `0x03`=currency (`[u32: code_len][code_bytes]`) |
| `0x06` | Date/Time | `[u32: var_name_len][var_name_bytes][u8: style]` where style: `0x00`=date, `0x01`=time, `0x02`=datetime |
| `0x07` | Variable w/ Default | `[u32: name_len][name_bytes][u32: default_len][default_bytes]` — writes param value if present, default otherwise |
| `0x08` | Relative Time | `[u32: var_name_len][var_name_bytes][u8: style]` where style: `0x00`=auto, `0x01`=seconds, `0x02`=minutes, `0x03`=hours, `0x04`=days, `0x05`=weeks, `0x06`=months, `0x07`=years |
| `0x09` | List Format | `[u32: var_name_len][var_name_bytes][u8: style]` where style: `0x00`=conjunction (and), `0x01`=disjunction (or), `0x02`=unit (commas only) |
| `0x0A` | Ordinal Plural | Same encoding as `0x03` but selects from CLDR ordinal rules instead of cardinal |
| `0x0B` | Escaped Variable | `[u32: var_name_len][var_name_bytes][u8: flags]` — bit 0 = raw (no HTML escape) |
| `0x0C` | Variable w/ Default (escaped) | Same as `0x07` + `[u8: flags]` |
| `0x0D` | Custom / MF2 function | `[u32: var_len][var][u32: lit_len][literal][u32: fmt_len][fmt][u32: opt_len][options]` — built-in `:test:*` when `fmt` is `test:function`, `test:select`, or `test:format` |
| `0x0E` | MF2 Match | `[u8: sel_count][selectors…][u16: input_count][inputs…][u16: local_count][locals…][u16: variant_count][variants…]` — runtime `.match` with `:test:*` selector resolution; each decl/variant key is length-prefixed UTF-8 |

## Index Format (hash keys)

The inner `L10N` block uses a sorted u64 hash index for O(log N) binary search lookup:

### L10N v1 header (legacy, still accepted)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `L10N` |
| 4 | 4 | Format version `1` |
| 8 | 4 | Index offset (byte offset of index from block start) |
| 12 | 4 | Index count (number of entries) |
| 16 | * | Data pool (bytecode values only) |
| index_offset | count * 16 | Index entries |

### L10N v2 header

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `L10N` |
| 4 | 4 | Format version `2` |
| 8 | 4 | `min_runtime_version` — runtime rejects if `RUNTIME_VERSION < min_runtime_version` |
| 12 | 4 | Index offset |
| 16 | 4 | Index count |
| 20 | * | Data pool |
| index_offset | count * 16 | Index entries |

### L10N v3 header (current compiler output)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `L10N` |
| 4 | 4 | Format version `3` |
| 8 | 4 | `min_runtime_version` — runtime rejects if `RUNTIME_VERSION < min_runtime_version` |
| 12 | 4 | `locale_data_version` — runtime rejects if `SUPPORTED_LOCALE_DATA_VERSION < locale_data_version` |
| 16 | 4 | Index offset |
| 20 | 4 | Index count |
| 24 | * | Data pool |
| index_offset | count * 16 | Index entries |

`locale_data_version` pins the CLDR-lite tables (plural, number, date, RTL) used at compile time. Bump `LOCALE_DATA_VERSION` in the compiler when those tables change incompatibly; ship a runtime that understands the new revision before loading newer paks.

Optional trailing `DBGK` section (dev builds with `debug-keys` feature): hash → UTF-8 key name table for debugging misses.

## Runtime formatting parameters

| Param | Purpose |
|-------|---------|
| `tz` | IANA timezone or fixed offset (`America/Bogota`, `+05:30`, `UTC`) for date/time opcode `0x06`. Values are parsed as UTC instants (`Z` or offset suffix); display is shifted into the requested zone (standard offsets only, no DST). |

## Bidirectional text

For RTL locales (`ar`, `he`, `fa`, `ur`, …), the formatter wraps embedded LTR runs (ASCII alphanumerics, URLs) with Unicode first-strong isolates (`U+2068` / `U+2069`) so mixed-script messages render correctly.

### Modular bundle layout

When `"bundles": { "mode": "modular" }` in `l10n4x.config.json`, the CLI emits:

```
outputDir/
  namespaces.json
  en/
    common.pak
    auth.pak
```

`namespaces.json` lists available namespaces per locale and optional `preload` list for `init_modular()`.

Each index entry (16 bytes):

| Offset | Size | Field |
|--------|------|-------|
| 0 | 8 | FNV-1a 64-bit key hash (big-endian) |
| 8 | 4 | Value bytecode offset (from block start) |
| 12 | 4 | Value bytecode length |

Key names are not stored in the binary. The index is sorted by hash ascending for binary search. Hashes are FNV-1a 64-bit computed at compile time. Runtime uses the hash for lookup directly. No string comparison, no key strings in memory.

## Compile-time validation

Templates are fully validated when building `.pak` files. Invalid MF2 syntax or data-model violations fail the build with locale + key. Runtime assumes bytecode is valid.