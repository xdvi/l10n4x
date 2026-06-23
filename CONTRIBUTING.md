# Contributing

Thank you for considering contributing to l10n4x. This document covers the development setup, coding conventions, testing practices, and PR workflow.

---

## Development Setup

### Prerequisites

- Rust 1.82 or later (MSRV). Install via [rustup](https://rustup.rs/).
- A wasm32 target (for WASM bindings):
  ```bash
  rustup target add wasm32-unknown-unknown
  ```

### Build

```bash
# Build all workspace packages
cargo build --workspace

# Build with release optimizations
cargo build --release

# Build a specific package
cargo build -p l10n4x-core
cargo build -p l10n4x-compiler
cargo build -p l10n4x-toolkit
```

### Test

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific package
cargo test -p l10n4x-core

# Run tests with all features
cargo test --workspace --all-features
```

### Lint

```bash
# Clippy (must pass cleanly)
cargo clippy --workspace -- -D warnings

# Format check
cargo fmt --all --check
```

### Generate signing key (required for build/dev commands)

```bash
# Generate a 32-byte Ed25519 seed as 64 hex chars
head -c 32 /dev/urandom | xxd -p -c 64
export L10N4X_SIGNING_KEY="<the-64-char-hex>"
```

### Run the CLI

```bash
cargo run -- init          # Interactive config wizard
cargo run -- build         # Compile .pak files
cargo run -- dev           # Hot-reload dev server
```

---

## Coding Conventions

### Rust

- **MSRV 1.82** -- do not use language features stabilized after Rust 1.82.
- **`no_std` in core** -- `l10n4x-core` must remain `#![no_std]`-compatible. Only `core` and `alloc` are available by default; `std` is gated behind the `std` feature.
- **`unsafe` discipline** -- every `unsafe` block must have a comment on the preceding line with `// SAFETY:` explaining why the invariants hold. The crate uses `#![deny(unsafe_op_in_unsafe_fn)]`.
- **Clippy** -- all code must pass `cargo clippy --workspace -- -D warnings`. No exceptions.
- **Formatting** -- use `cargo fmt`. Editor config: `max_width=100`, `imports_granularity="Crate"`, `group_imports="StdExternalCrate"`.
- **Dependencies** -- prefer `default-features = false` on all dependencies; opt in only to needed features.
- **Error handling** -- structured error types where possible (prefer enums over `&'static str`). In `l10n4x-core`, `&'static str` is acceptable for infallible or memory-constrained paths.
- **Public API** -- all public items must have doc comments (`#![warn(missing_docs)]` in core). Document panics, errors, and safety invariants.

### Features

- `l10n4x-core` features: `default = ["std"]`, `std` implies `alloc` + `encryption` + `crossbeam-epoch`, `alloc` implies `ed25519-dalek`.
- New dependencies must follow the existing feature-gating pattern.
- Do not add dependencies that pull in `std` when `no_std` is requested.

### Tests

- **Unit tests** -- use `#[cfg(test)]` modules inside each source file. Test private functions directly.
- **Integration tests** -- placed in `tests/` at the package level. Test public APIs.
- **Snapshot tests** -- target generators use snapshot testing to verify generated output contains expected strings.
- **Compile-fail tests** -- for soundness guarantees, use `trybuild` (see `packages/core/tests/compile_fail/`).
- **Test naming** -- use descriptive snake_case names: `test_` prefix, e.g. `test_lookup_returns_none_for_missing_locale`.
- **Coverage** -- aim for >80% line coverage on all modules. Untested modules block PRs.

---

## Project Structure

```
l10n4x/
  packages/
    core/          -- l10n4x-core (no_std runtime)
    compiler/      -- l10n4x-compiler (build-time)
    cli/           -- l10n4x-toolkit (CLI binary)
    ffi/           -- l10n4c (C FFI)
    wasm/          -- l10n4x-wasm (WASM bindings)
  docs/            -- Documentation
  locales/         -- Source translation JSON files
  examples/        -- Framework integration examples
  scripts/         -- Scripts for CI and verification
```

### How to add a new target generator

1. **Create a new file** in `packages/cli/src/targets/<name>.rs` with a `pub fn generate(...)` function following the `GenerateContext` pattern from existing generators.

2. **Register the module** in `packages/cli/src/targets/mod.rs`:
   ```rust
   pub mod <name>;
   ```

3. **Add a dispatch arm** in `packages/cli/src/generator.rs` inside `generate_bindings()`:
   ```rust
   "<name>" => {
       targets::<name>::generate(out_dir, &sorted_keys, &target.options, &ctx)?;
   }
   ```

4. **Register the target** in `packages/cli/src/main.rs`:
   - Add it to `detect_project_type()` if auto-detection is possible.
   - Add it to `init_wizard()` if it should be available in `l10n4x init`.

5. **Add snapshot tests** in the new file to verify generated output.

6. **Run all tests and clippy** before submitting.

---

## PR Process

1. **Fork** the repository on GitHub.

2. **Create a branch** from `main`:
   ```bash
   git checkout -b feat/my-feature
   ```

3. **Make your changes.** Keep commits small and focused. Each commit should compile and pass tests independently.

4. **Write or update tests.** Cover new functionality, edge cases, and error paths.

5. **Run verification**:
   ```bash
   cargo test --workspace
   cargo clippy --workspace -- -D warnings
   cargo fmt --all --check
   ```

6. **Squash commits** into logical units (typically 1 commit per task):
   ```bash
   git rebase -i main
   ```

7. **Open a pull request** against `main`. Provide:
   - A clear title and description
   - Reference to any related issues
   - Notes on breaking changes or migration considerations

8. **CI must pass** before a review is requested. The CI pipeline runs:
   - `cargo test --workspace`
   - `cargo clippy --workspace -- -D warnings`
   - `cargo fmt --all --check`
   - `cargo build` for wasm32 target
   - Linux, macOS, and Windows builds
   - Coverage report (cargo-tarpaulin)
   - cargo-audit (scheduled)

9. **Peer review.** At least one maintainer must approve. Address all feedback.

10. **Merge.** Maintainers will squash-merge into `main` once approved and CI is green.

---

## Release Process

1. Update version in workspace `Cargo.toml`.
2. Update `CHANGELOG.md`.
3. Tag the release: `git tag v0.2.0 && git push --tags`.
4. CI builds release artifacts and publishes to GitHub Releases.
5. Publish to crates.io: `cargo publish -p l10n4x-core && cargo publish -p l10n4x-compiler && cargo publish -p l10n4x-toolkit && cargo publish -p l10n4c && cargo publish -p l10n4x-wasm`.

---

## Code of Conduct

This project follows the [Rust Code of Conduct](https://www.rust-lang.org/policies/code-of-conduct). Please be respectful, inclusive, and constructive in all interactions.

---

## Getting Help

- Open an issue on GitHub for bugs or feature requests.
- Discuss architecture decisions in issues before opening large PRs.
- For questions about the `.pak` format, see `docs/PAK_FORMAT.md`.
- For the threat model and security considerations, see `docs/THREAT_MODEL.md`.
- For enterprise adoption patterns, see `docs/ENTERPRISE_ADOPTION.md`.
