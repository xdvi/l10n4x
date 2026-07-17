# Changelog

All notable changes to this project will be documented in this file.

This project uses [release-please](https://github.com/googleapis/release-please) to
automate releases. Entries below are generated from conventional commits.

---

## [1.0.0](https://github.com/xdvi/l10n4x/compare/v0.1.0...v1.0.0) (2026-07-17)


### ⚠ BREAKING CHANGES

* magic bytes L10P -> L10K, extension .pak -> .lpk, module core::pak -> core::lpk, all pak-named APIs renamed to lpk (FFI exports included). Old 12-byte header parsing removed; only the 16-byte flagged header is accepted. Old .pak files are not readable and must be recompiled.

### Bug Fixes

* **compiler:** judgment-day round 1 — mutex poison resilience, error propagation, array-of-objects flattening ([955a27e](https://github.com/xdvi/l10n4x/commit/955a27e1a0beb516e6b1583069db0f670178ae15))
* **core,ffi,cli:** harden edge cases — overflow checks, callback UAF, torn AES key ([d9e69ae](https://github.com/xdvi/l10n4x/commit/d9e69ae25c240ee28d4389d7da4b13f985bc3b48))
* **core,ffi,wasm,cli:** runtime correctness round — stale caches, lost updates, panic guards ([f25ff59](https://github.com/xdvi/l10n4x/commit/f25ff59896ecfef098f3381e8ed807d81a700d60))
* **deps:** bump crossbeam-epoch to 0.9.20 for RUSTSEC-2026-0204 ([#30](https://github.com/xdvi/l10n4x/issues/30)) ([21af61d](https://github.com/xdvi/l10n4x/commit/21af61dd28749c34ccfe7d8457946e821953d22a))
* repair CI regressions from perf-optimizations round (clippy, no_std, fmt) ([0146dc6](https://github.com/xdvi/l10n4x/commit/0146dc6b1631237b0710cdd54fa740b9174ae7eb))


### Performance Improvements

* **compiler,core:** reduce allocations and parallelize compile pipeline ([2a91a6b](https://github.com/xdvi/l10n4x/commit/2a91a6b0aecb7ef32efb191d3a92ab918caaf20e))
* **compiler:** combine flatten and parse into single pass with Mutex error handling ([a741a8f](https://github.com/xdvi/l10n4x/commit/a741a8f75ec4316aef4aff8f6d4d73f96e00d87d))
* **compiler:** parallelize locale processing with rayon ([792d691](https://github.com/xdvi/l10n4x/commit/792d691021b935488e22af5a3960d27810cd5fcf))
* **compiler:** pre-dimension HashMap and Vec with with_capacity ([f28c798](https://github.com/xdvi/l10n4x/commit/f28c79822ab9958d2c4b177d85a08d892fb2643a))
* **compiler:** refactor binary_writer to use Write trait for zero-copy serialization ([e02ed12](https://github.com/xdvi/l10n4x/commit/e02ed1243b6a472dd215806ff1157060b3c29b93))
* **compiler:** switch HashMap to ahash for faster hashing ([b43cc58](https://github.com/xdvi/l10n4x/commit/b43cc583bc803ae62cf7c0985486249f8b49687f))
* **compiler:** use serde_json::from_reader to avoid intermediate String allocation ([b74520c](https://github.com/xdvi/l10n4x/commit/b74520c621cd04f8e107b026aae94ffdc5065eb2))
* **compiler:** use streaming zstd encoder to reduce peak memory ([444c4f5](https://github.com/xdvi/l10n4x/commit/444c4f5789071d5b1e785cfba346efc7c68559b4))
* **core:** stop re-allocating MF2 render state; refactor(core): error-path debt ([f468419](https://github.com/xdvi/l10n4x/commit/f468419636b78ddc8df7c32fdc3644f8f5fbcb58))


### Refactors

* rename .pak format to .lpk, drop legacy format support ([8fe10fb](https://github.com/xdvi/l10n4x/commit/8fe10fb6966651d0574a389ecab23c258a7e425d))

## [0.1.0] - 2026-06-29

### Initial Release

- **Core runtime** (`l10n4x-core`): `#![no_std]` compatible, sorted binary lookup
  (O(log N)), zero-allocation ICU MessageFormat 1.0/2.0 formatting, lock-free RCU
  hot-reload, scoped multi-tenant store handles, EBR-based memory reclamation.
- **Compiler** (`l10n4x-compiler`): JSON/YAML-to-`.pak` pipeline with MF2 parsing,
  compile-time validation, interval plural ranges, and key-reference inlining.
- **Toolkit CLI** (`l10n4x-toolkit`): `build`, `dev`, `init`, `sync`, `check`
  commands with hot-reload dev server (Axum), TMS integration, and binding
  generation for Go, Python, C, TypeScript, Flutter, and C#.
- **C FFI** (`l10n4c`): Memory-safe runtime bindings with UTF-8 validation,
  buffer overflow guards, scoped store API, and OTA reload/rollback.
- **WASM** (`l10n4x-wasm`): Browser/Node.js bindings with JS error propagation
  and `wasm-pack` support.
- **Signing & encryption**: Ed25519-signed `.pak` integrity with optional
  AES-GCM encryption envelope (`L10E`).
- **Enterprise-ready CI**: SHA-pinned actions, harden-runner, branch protection,
  CODEOWNERS, Dependabot groups, semver-checks, conventional commits enforcement,
  cargo-audit/deny, release-please, and crates.io publishing.

[0.1.0]: https://github.com/xdvi/l10n4x/releases/tag/v0.1.0
