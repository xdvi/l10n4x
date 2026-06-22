# TMS Integration

Exchange locale JSON with translation teams and push signed `.pak` artifacts to enterprise endpoints.

## Providers

| Provider | Export | Import | Push |
|----------|--------|--------|------|
| `file` | `l10n4x-tms.json` bundle | merge into `sourceDir` | — |
| `crowdin` | `locale/namespace.json` tree | from download dir | manual via Crowdin UI |
| `webhook` | — | — | POST signed paks (base64 + SHA-256) |

## Configuration

Add to `l10n4x.config.json`:

```json
{
  "tms": {
    "provider": "webhook",
    "webhookUrl": "https://cdn.example.com/l10n/ingest",
    "webhookTokenEnv": "L10N4X_WEBHOOK_TOKEN",
    "pushOnBuild": true
  }
}
```

## Commands

```bash
# Export portable bundle for offline TMS handoff
l10n4x sync --provider file --direction export --out ./tms-export

# Import translated bundle back into source JSON
l10n4x sync --provider file --direction import --from ./tms-export

# Crowdin-compatible directory (upload per locale file)
l10n4x sync --provider crowdin --direction export --out ./tms-crowdin

# Import Crowdin download directory
l10n4x sync --provider crowdin --direction import --from ./crowdin-download

# Push signed paks after build (or standalone)
l10n4x build
l10n4x sync --provider webhook --direction push
```

## Exchange format (`l10n4x-tms.json`)

```json
{
  "format": "l10n4x-tms",
  "version": 1,
  "project": "my-app",
  "fallback": "en",
  "exportedAt": "1719062400",
  "namespaces": {
    "common": {
      "en": { "welcome.title": "Welcome" },
      "es": { "welcome.title": "Bienvenido" }
    }
  }
}
```

## Webhook payload

```json
{
  "project": "my-app",
  "pushedAt": "1719062400",
  "bundleMode": "modular",
  "artifacts": [
    {
      "locale": "en",
      "namespace": "common",
      "path": "dist/en/common.pak",
      "sha256": "…",
      "pakBase64": "…"
    }
  ]
}
```

Runtime consumers must still verify Ed25519 signatures — the webhook digest is for transport integrity only.

## Related

- [ENTERPRISE_ADOPTION.md](./ENTERPRISE_ADOPTION.md) — roles and CI/CD
- [ROADMAP.md](./ROADMAP.md) — future Crowdin API automation