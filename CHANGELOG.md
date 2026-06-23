# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

## [0.2.0] - 2026-06-22

### Added
- **Hardened FFI Layer**: Enforced UTF-8 encoding checks (`L10N4C_INVALID_ENCODING = 6`) and buffer overflow boundaries (`L10N4C_BUFFER_OVERFLOW = 12`) on all string parameters and raw pointer calculations.
- **FFI bindgen Synchronization**: Integrated an automated test verifying numerical alignment between Rust FFI error constants and `l10n4c.h` C macros.
- **P0 thread-safe reload**: `load`/`clear`/`swap_store` writers serialized via mutex; readers remain lock-free RCU.
- **P0 modular bundles**: opt-in `"bundles": { "mode": "modular" }` emits `{locale}/{namespace}.pak` plus `namespaces.json`; runtime `load_namespace` / `init_modular` APIs (core + FFI).
- **P0 debug keys**: optional `debug-keys` feature embeds `DBGK` hash→name table; CLI `validate --report-misses` shows expected source file paths.
- **P0 L10N v2**: `min_runtime_version` header field; `RuntimeTooOld` error (`L10N4C_RUNTIME_TOO_OLD = 13`).
- **P1.1 OTA translation updates**: `try_ota_reload_pak` / `try_ota_rollback` with one retired snapshot per locale; FFI `l10n4c_ota_*`; metrics `pak_reload_total`, `pak_verify_failures`, `pak_rollback_total`.
- **P1.2 Fine-grained COW locales**: per-entry `Arc<StoreData>` sharing via `upsert_locale` / `remove_locale` (no whole-`Vec` clone on single-locale reload).
- **P1.3 Hot-path parity**: public `hash_params` in core; FFI/WASM thread-local caches check before param hash on empty params; WASM uses `translate_to_writer_with_status`.
- **P1.4 Production observability**: v2 `metrics_string` (`cache_hit_ratio`, `miss_by_locale`, OTA counters); optional `tracing` feature; CI bench regression script (5% threshold).
- **P1.5 Test hardening**: wasmtime smoke test for WASM; interval plural compile→translate E2E; strengthened `l10n4c_get_loaded_locales` FFI test; dev server 15s exponential backoff.
- **Enterprise adoption guide**: `docs/ENTERPRISE_ADOPTION.md` — governance, CI/CD, namespace ownership, OTA (Angular/SAP-style patterns).
- **P2.6 JS bridge**: WASM exports `l10n4x_load_namespace_bytes`, `l10n4x_ota_reload_pak`, `l10n4x_ota_rollback`, `l10n4x_ota_can_rollback`.
- **P2.1 TMS integration**: `l10n4x sync --provider file|crowdin|webhook` with export/import/push; optional `tms.pushOnBuild` webhook after `build`.
- **TMS docs**: `docs/TMS.md` — exchange format and webhook payload.

### Changed
- **Architectural Signing Key Removal**: Signing capabilities moved completely out of the runtime `core` package into the build-time `compiler` crate, preventing signing keys from being exposed in runtime client bundles.
- **Epoch-Based Memory Reclamation (EBR)**: Replaced raw spinlock memory pooling in `TranslationStore` with standard `crossbeam-epoch` concurrent reclamation, and a panic-safe `AtomicUsize` re-entrancy guard for `no_std` environments.
- **Dev Server Security**: Secured the dev server with customizable CORS origins validation (dynamic localhost fallback, rejecting `null` origins), SSE event and payload raw newline sanitization, and timing-attack resistant constant-time Axios authentication token checks under selective Axum sub-routers.
- Compiler emits L10N format v2 by default; v1 paks remain readable.
- `l10n4c_get_metrics` returns v2 extended format (first five numeric fields remain backward-compatible).
- Web runtime (`@l10n4x/react`, `@l10n4x/runtime`) lives in the separate [l10n4x-js](https://github.com/xdvi/l10n4x-js) repository.

## [0.1.0] - 2026-06-20

### Added
- **Core Runtime (`l10n4x-core`)**: High-performance, `#![no_std]` compatible runtime featuring sorted binary lookup (O(log N)) and zero-allocation ICU MessageFormat 1.0/2.0 formatting (plurals, select, variables).
- **Integrity Layer (`l10p` / `l10e`)**: Cryptographic pack format using Ed25519 signatures for package sealing and optional AES-GCM encryption envelopes.
- **C-Compatible FFI (`l10n4c`)**: Runtime-only dynamic library bindings for loading signed `.pak` packages, verifying signatures, decrypting data, and performing thread-safe lookups from any language with C-FFI support.
- **Toolkit CLI (`l10n4x-toolkit`)**: Command-line compiler that transforms translation templates into signed `.pak` files, and generates Go, TypeScript, and C# type-safe wrappers.
- **WebAssembly Bindings (`l10n4x-wasm`)**: WASM integration wrapper allowing the translation engine to run in browsers and Node.js environments.
- **Structured Compiler Errors**: `thiserror`-based `CompileError` enum with granular error variants instead of opaque static string slices.
- **WASM Exception Propagation**: WebAssembly bindings throw descriptive JavaScript `Error` objects on invalid format, decompression, or key verification failures.
- **TypeScript SSR Support**: Isomorphic `PakLoader` architecture with injectable loaders (`fetchPakLoader` for browsers, `fsPakLoader` for Node.js/SSR, `autoPakLoader` for auto-detection). Includes Next.js App Router integration examples.
- **Wrapper Examples**: Multi-language integration examples for Go, Python, C#, Flutter, and TypeScript (client + server/SSR).
- **Smoke Tests in CI/CD**: Full integration smoke test suite covering Python, Go, C#, Flutter, and TypeScript against remote runners.

### Architecture
- **RCU-Safe Fallback Locale**: The fallback locale is a field of `TranslationStore`, protected under the main `STORE` RCU pointer. No global atomic pointer, no spin-wait deadlocks, no UAF.
- **Zero-Copy Locale Buffers**: `TranslationStore` locale buffers use `Arc<Vec<u8>>`. Loading new locales performs cheap reference count updates instead of deep-cloning all previously loaded binary data.
- **Runtime-Only FFI Surface**: The C-FFI layer (`l10n4c`) exposes only runtime operations (load `.pak`, translate, clear). Compilation is exclusively handled by the CLI, enforcing integrity by architecture — there is no way to load unsigned content through the public API.
