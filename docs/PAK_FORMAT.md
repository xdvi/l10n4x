# `.pak` File Format (v1)

All multi-byte integers are **big-endian**.

## Signed container (`L10P`)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `L10P` |
| 4 | 4 | Version `1` |
| 8 | 4 | Payload length (N) |
| 12 | N | DEFLATE-compressed inner `L10N` binary |
| 12+N | 64 | Ed25519 signature over bytes `[0..12+N)` |

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