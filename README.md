# l10n4x

[![Language](https://img.shields.io/badge/language-Rust-orange.svg)](https://www.rust-lang.org/)
[![C-FFI](https://img.shields.io/badge/C--FFI-Go%20%7C%20C%2B%2B%20%7C%20C%23%20%7C%20Dart-blue.svg)](#)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](#)
[![CI](https://github.com/xdvi/l10n4x/actions/workflows/ci.yml/badge.svg)](https://github.com/xdvi/l10n4x/actions/workflows/ci.yml)

> *Modern, Dynamic, and Type-Safe Localization (l10n) Engine and Toolkit in Rust.*

`l10n4x` compiles translation bundles into compressed `.pak` files, loads them dynamically in any runtime, and generates **type-safe bindings** for Go, TypeScript/React, C/C++, Python, and Flutter/Dart.

---

## Install

Download prebuilt binaries from [GitHub Releases](https://github.com/xdvi/l10n4x/releases/latest). Each platform bundle includes `l10n4x`, `l10n4c` (shared library), and `l10n4c.h` with standard names:

| Platform | Bundle |
|----------|--------|
| Linux | `l10n4x-linux-amd64.tar.gz` |
| macOS | `l10n4x-macos-universal.tar.gz` |
| Windows | `l10n4x-windows-amd64.zip` |
| WASM | `l10n4x.wasm` |

No Rust toolchain required for consumers. Building from source is optional (for contributors).

### Examples

| Example | Path |
|---------|------|
| Go | [`examples/go`](examples/go) |
| Python | [`examples/python`](examples/python) |
| C# | [`examples/csharp`](examples/csharp) |
| Flutter | [`examples/flutter`](examples/flutter) |
| TypeScript (SSR) | [`examples/typescript`](examples/typescript) |

Smoke-test all examples (builds `l10n4c`, compiles fixture paks, runs each binding):

```bash
./scripts/verify.sh
```

---

## Toolkit

| Command | Description |
|---------|-------------|
| `l10n4x build` | Compile `.pak` files and generate bindings (CI-safe) |
| `l10n4x build --dry-run` | Validate keys without writing output |
| `l10n4x dev` | Hot-reload dev server |
| `l10n4x validate` | Check key consistency |

### JSON flattening rules

Nested objects are flattened with dot notation. Arrays follow two distinct rules:

**Primitive arrays** (strings, numbers, booleans) are stored as a single JSON literal at the array key:
```json
{ "menu": { "items": ["Home", "Settings"] } }
```
→ `menu.items` = `["Home","Settings"]`

**Object arrays** require semantic keys inside each element — numeric index keys are not supported:
```json
{ "menu": { "items": [{ "home": "Home" }, { "settings": "Settings" }] } }
```
→ `menu.items.home` = `Home`, `menu.items.settings` = `Settings`

---

## C-FFI: `l10n4c`

Header: [`packages/ffi/l10n4c.h`](packages/ffi/l10n4c.h)

Interpolation uses typed `L10n4cParam { key, value }` arrays — no JSON parsing in the FFI layer.

```c
L10n4cParam params[] = { { "name", "Diego" } };
char *out = l10n4c_translate_with_params_alloc("en", "welcome", params, 1);
l10n4c_free_string(out);
```

`.pak` files are **signed** (Ed25519, mandatory). Optional AES-GCM encryption (`"encrypt": true`) wraps the signed pak for confidentiality — see [docs/PAK_FORMAT.md](docs/PAK_FORMAT.md) and [docs/THREAT_MODEL.md](docs/THREAT_MODEL.md).

---

## Documentation

| Document | Description |
|----------|-------------|
| [Enterprise Adoption](docs/ENTERPRISE_ADOPTION.md) | Governance, CI/CD, namespaces, OTA |
| [Architecture](docs/ARCHITECTURE.md) | Data flow and package layout |
| [Roadmap](docs/ROADMAP.md) | P2 backlog and shipped milestones |
| [l10n4x-js](https://github.com/xdvi/l10n4x-js) | Official `@l10n4x/react` and `@l10n4x/runtime` packages |

---

## Testing

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo bench -p l10n4x-core
```