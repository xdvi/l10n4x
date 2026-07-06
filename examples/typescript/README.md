# l10n4x — TypeScript examples

Demonstrates how to use `l10n4x-wasm` from TypeScript in **browser** and **Node.js / SSR** environments.

## Structure

```
typescript/
├── i18n.ts          # Isomorphic core — environment-agnostic, loader-injected
├── client.ts        # Browser entry point (Fetch API)
├── server.ts        # Node.js / SSR entry point (fs/promises)
├── demo-browser.ts  # Interactive browser demo
├── demo-node.ts     # Runnable Node.js demo
└── index.html       # HTML shell for the browser demo
```

## Architecture

The key design is **loader injection**: `i18n.ts` does not hardcode how `.lpk`
files are fetched. Instead, it accepts a `LpkLoader` function:

```ts
type LpkLoader = (locale: string) => Promise<Uint8Array>;
```

This makes the core isomorphic — the same `createI18n()` factory works in every
environment by supplying the right loader.

| Environment | Entry point | Loader |
|---|---|---|
| Browser / CDN | `client.ts` | `browserLpkLoader(baseUrl)` — uses `fetch` |
| Node.js / SSR | `server.ts` | `nodeLpkLoader(dir)` — uses `fs/promises` |
| Custom / Edge | `i18n.ts` | inject your own `LpkLoader` |

## Quick start

### Node.js demo

```bash
npm install
npm run demo:node
```

### Browser demo (requires Vite)

```bash
npm install
npm run demo:browser
```

> Ensure `.lpk` files are in `../dist/locales/` (relative to this directory),
> or adjust the path in `demo-node.ts` / `demo-browser.ts`.

## Usage in Next.js (App Router)

### 1. Configure `next.config.mjs`

```js
// next.config.mjs
export default {
  experimental: {
    serverExternalPackages: ["l10n4x-wasm"],
  },
};
```

### 2. Create a shared singleton

```ts
// lib/i18n.ts
import { createServerI18n, type I18nInstance } from "@/examples/typescript/server";

let _instance: I18nInstance | undefined;

export async function getI18n(): Promise<I18nInstance> {
  if (!_instance) {
    _instance = await createServerI18n({
      localesDir: "./public/locales",
      fallbackLocale: "en",
      preloadLocales: ["en", "es"],
    });
  }
  return _instance;
}
```

### 3. Use in a Server Component

```tsx
// app/[locale]/page.tsx
import { getI18n } from "@/lib/i18n";

export default async function Page({ params }: { params: { locale: string } }) {
  const i18n = await getI18n();
  return <h1>{i18n.t(params.locale, "app.title")}</h1>;
}
```

### 4. Use in a Route Handler

```ts
// app/api/translate/route.ts
import { getI18n } from "@/lib/i18n";

export async function GET(req: Request) {
  const { searchParams } = new URL(req.url);
  const locale = searchParams.get("locale") ?? "en";
  const key   = searchParams.get("key") ?? "";
  const i18n  = await getI18n();
  return Response.json({ text: i18n.t(locale, key) });
}
```

## Generated code (CLI output)

The CLI target `typescript` emits a thin `generated.ts` with typed key hashes only
(`packages/cli/src/templates/ts_keys.ts`). Runtime and loaders live in
[`@l10n4x/runtime`](https://github.com/xdvi/l10n4x-js):

```ts
import { createL10n } from "@l10n4x/runtime";
import { Keys } from "./generated";

const l10n = createL10n({
  outputDir: "/locales",
  verifyPublicKey: "…64 hex…",
  fallback: "en",
});

await l10n.initialize();
await l10n.loadLocale("en");
l10n.t("en", Keys.WelcomeTitle);
```
