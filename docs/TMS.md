# TMS Integration

Exchange locale JSON with translation teams and push signed `.pak` artifacts to enterprise endpoints.

## Architecture

| Layer | Providers |
|-------|-----------|
| **Core** (built into `l10n4x`) | `file`, `webhook` |
| **Plugins** (optional install) | `crowdin` → `l10n4x-plugin-crowdin` |

```bash
l10n4x plugin list              # core + optional plugins
l10n4x plugin info crowdin      # install + contract hints
l10n4x plugin validate crowdin  # lint plugin binary/CLI (alias: plugin lint)
l10n4x plugin validate          # all discovered plugins
```

### Plugin contract (for authors)

| Rule | Requirement |
|------|-------------|
| Binary name | `l10n4x-plugin-<id>` on `PATH` |
| Plugin id | `[a-z][a-z0-9-]*`, not `file` or `webhook` |
| Sync CLI | `sync export\|import\|push --config l10n4x.config.json [--out] [--from]` |
| Help | `--help` exits 0 and mentions `sync` |
| Info | `info` subcommand recommended |
| Config | optional `plugins.<id>` in `l10n4x.config.json` |
| Exit code | `0` success, non-zero on error |

A shell script is valid if it is executable and implements the CLI above.

## Core providers

| Provider | Export | Import | Push |
|----------|--------|--------|------|
| `file` | `l10n4x-tms.json` bundle | merge into `sourceDir` | — |
| `webhook` | — | — | POST signed paks (base64 + SHA-256) |

## Crowdin plugin

Install (optional — linked by default in official builds):

```bash
cargo install l10n4x-plugin-crowdin
```

Config:

```json
{
  "plugins": {
    "crowdin": {
      "projectId": "12345",
      "tokenEnv": "CROWDIN_TOKEN"
    }
  },
  "tms": {
    "provider": "crowdin"
  }
}
```

| Direction | Today | Planned |
|-----------|-------|---------|
| `export` | `locale/namespace.json` tree | same |
| `import` | `--from <download-dir>` | API pull when `projectId` + `tokenEnv` set |
| `push` | manual export + Crowdin UI | API upload |

## Webhook configuration

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
# Export portable bundle for offline TMS handoff (core)
l10n4x sync --provider file --direction export --out ./tms-export

# Import translated bundle back into source JSON (core)
l10n4x sync --provider file --direction import --from ./tms-export

# Crowdin-compatible directory (plugin)
l10n4x sync --provider crowdin --direction export --out ./tms-crowdin

# Import Crowdin download directory (plugin)
l10n4x sync --provider crowdin --direction import --from ./crowdin-download

# Push signed paks after build (core)
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
- [ROADMAP.md](./ROADMAP.md) — Crowdin API automation (plugin backlog)