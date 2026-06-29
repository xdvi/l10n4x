# l10n4x-plugin-crowdin

Optional Crowdin TMS plugin for [l10n4x](https://github.com/xdvi/l10n4x).

## Install

```bash
cargo install l10n4x-plugin-crowdin
```

Official `l10n4x` builds link this plugin by default. Minimal builds:

```bash
cargo install l10n4x-toolkit --no-default-features
cargo install l10n4x-plugin-crowdin
```

## Config (`l10n4x.config.json`)

```json
{
  "plugins": {
    "crowdin": {
      "projectId": "12345",
      "tokenEnv": "CROWDIN_TOKEN"
    }
  }
}
```

## Usage

```bash
l10n4x sync --provider crowdin --direction export --out ./tms-crowdin
l10n4x sync --provider crowdin --direction import --from ./crowdin-download
```

Or run standalone:

```bash
l10n4x-plugin-crowdin sync export --config l10n4x.config.json --out ./tms-crowdin
```