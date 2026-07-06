# Threat Model

## Protected

| Threat | Mitigation |
|--------|------------|
| Tampered `.lpk` in transit or on disk | Ed25519 signature verified before decompression |
| Malformed input | Magic, version, length, and signature checks |

## Not protected (by design)

| Asset | Rationale |
|-------|-----------|
| Translation confidentiality (default) | UI strings are public in client bundles |
| Signing seed secrecy in CI | Operational concern — use a secret manager for `L10N4X_SIGNING_KEY` |
| AES key extractability in client | Any key embedded or loaded in a client binary can be recovered by a determined attacker |

## Optional encryption (`encrypt: true`)

AES-256-GCM (`L10E` envelope) is **opt-in** for teams that need confidentiality in transit or at rest (e.g. unreleased feature strings, sector compliance). It does **not** protect against reverse engineering: the decrypt key must be present in the client to load translations.

Use encryption only when you understand its limitations. Signature verification remains mandatory regardless.

## Key handling & Architecture Secrecy

- **Architecture Separation:** To minimize the attack surface, only the `compiler` crate and the `cli` tool have access to the signing seed. The runtime `core` package is entirely signature-verification only and has no capability to perform signing operations under any configuration.
- **Build:** `L10N4X_SIGNING_KEY` = 32-byte Ed25519 seed (never in repo, client, or the runtime core crate).
- **Runtime:** `verifyPublicKey` (hex) embedded in generated bindings — public by design.
- **Optional encrypt:** `L10N4X_ENCRYPT_KEY` = 32-byte AES key (build + runtime, only when `encrypt` is true).
- Re-sign all `.lpk` files when rotating the signing seed; update `verifyPublicKey` via `l10n4x build`.