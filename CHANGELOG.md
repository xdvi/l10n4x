# Changelog

All notable changes to this project will be documented in this file.

This project uses [release-please](https://github.com/googleapis/release-please) to
automate releases. Entries below are generated from conventional commits.

---

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
