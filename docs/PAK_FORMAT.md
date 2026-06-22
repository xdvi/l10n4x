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

**Optimization:** When a value is a single text node (no variables, plurals, etc.), it is stored as raw UTF-8 bytes without the `[0x01][len]` prefix. The runtime detects this at format time: any first byte `0x00` or `> 0x0D` is treated as raw text rather than an opcode.

| Opcode | Name | Encoding |
|--------|------|----------|
| `0x01` | Text | `[u32: len][len bytes: text]` |
| `0x02` | Variable | `[u32: var_name_len][var_name_bytes]` |
| `0x03` | Plural | `[u32: var_name_len][var_name_bytes][u16: case_count][cases...]` |
| `0x04` | Select | `[u32: var_name_len][var_name_bytes][u16: case_count][cases...]` |
| `0x05` | Number | `[u32: var_name_len][var_name_bytes][u8: style][style extras]` where style: `0x00`=decimal, `0x01`=percent, `0x02`=integer, `0x03`=currency (`[u32: code_len][code_bytes]`) |
| `0x06` | Date/Time | `[u32: var_name_len][var_name_bytes][u8: style]` where style: `0x00`=date, `0x01`=time, `0x02`=datetime |
| `0x07` | Variable w/ Default | `[u32: name_len][name_bytes][u32: default_len][default_bytes]` — writes param value if present, default otherwise |
| `0x08` | Relative Time | `[u32: var_name_len][var_name_bytes][u8: style]` where style: `0x00`=auto, `0x01`=seconds, `0x02`=minutes, `0x03`=hours, `0x04`=days, `0x05`=weeks, `0x06`=months, `0x07`=years |
| `0x09` | List Format | `[u32: var_name_len][var_name_bytes][u8: style]` where style: `0x00`=conjunction (and), `0x01`=disjunction (or), `0x02`=unit (commas only) |
| `0x0A` | Ordinal Plural | Same encoding as `0x03` but selects from CLDR ordinal rules instead of cardinal |

## Index Format (hash keys)

The inner `L10N` block uses a sorted u64 hash index for O(log N) binary search lookup:

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `L10N` |
| 4 | 4 | Version `1` |
| 8 | 4 | Index offset (byte offset of index from block start) |
| 12 | 4 | Index count (number of entries) |
| 16 | * | Data pool (bytecode values only) |
| index_offset | count * 16 | Index entries |

Each index entry (16 bytes):

| Offset | Size | Field |
|--------|------|-------|
| 0 | 8 | FNV-1a 64-bit key hash (big-endian) |
| 8 | 4 | Value bytecode offset (from block start) |
| 12 | 4 | Value bytecode length |

Key names are not stored in the binary. The index is sorted by hash ascending for binary search. Hashes are FNV-1a 64-bit computed at compile time. Runtime uses the hash for lookup directly. No string comparison, no key strings in memory.