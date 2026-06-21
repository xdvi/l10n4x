/**
 * server.ts — Node.js / SSR entry point for l10n4x
 *
 * Uses `node:fs/promises` to read .pak files from disk. Suitable for:
 *   - Next.js App Router (Server Components, Route Handlers)
 *   - Express / Fastify / Hono middleware
 *   - Standalone Node.js scripts
 *   - Edge runtimes that expose a Node-compatible `fs` shim
 *
 * ─── Next.js setup ────────────────────────────────────────────────────────
 *
 * 1. Add l10n4x-wasm to serverExternalPackages so Next.js does not try to
 *    bundle the native WASM module:
 *
 *      // next.config.mjs
 *      export default {
 *        experimental: {
 *          serverExternalPackages: ["l10n4x-wasm"],
 *        },
 *      };
 *
 * 2. Use createServerI18n() inside Server Components or Route Handlers:
 *
 *      // app/[locale]/page.tsx
 *      import { createServerI18n } from "@/examples/typescript/server";
 *      export default async function Page({ params }: { params: { locale: string } }) {
 *        const { locale } = params;
 *        const i18n = await getSharedI18n();        // see singleton pattern below
 *        return <h1>{i18n.t(locale, "app.title")}</h1>;
 *      }
 *
 * ─── Singleton pattern (recommended for servers) ──────────────────────────
 *
 * Avoid re-initialising on every request. Cache the I18nInstance at module
 * scope (Node.js module cache survives across requests in the same process):
 *
 *   let _i18n: I18nInstance | undefined;
 *   export async function getSharedI18n(): Promise<I18nInstance> {
 *     if (!_i18n) _i18n = await createServerI18n({ localesDir: "./dist/locales" });
 *     return _i18n;
 *   }
 */

import { createI18n, type I18nInstance, type I18nOptions, type PakLoader } from "./i18n";
import { join, resolve } from "node:path";
import { readFile } from "node:fs/promises";

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/**
 * Build a PakLoader backed by Node's `fs/promises`.
 *
 * @param localesDir  Absolute or relative path to the directory containing
 *                    `<locale>.pak` files.
 */
export function nodePakLoader(localesDir: string): PakLoader {
  const absDir = resolve(localesDir);
  return async (locale: string): Promise<Uint8Array> => {
    const filePath = join(absDir, `${locale}.pak`);
    try {
      const buf = await readFile(filePath);
      return new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
    } catch (err: unknown) {
      const msg =
        err instanceof Error ? err.message : String(err);
      throw new Error(
        `l10n4x: failed to read pak file for locale '${locale}' at '${filePath}': ${msg}`
      );
    }
  };
}

// ---------------------------------------------------------------------------
// Convenience factory
// ---------------------------------------------------------------------------

export interface ServerI18nOptions extends Omit<I18nOptions, "loader"> {
  /**
   * Directory containing .pak files.
   * @default "./dist/locales"
   */
  localesDir?: string;
  /** Custom loader (overrides localesDir). */
  loader?: PakLoader;
}

/**
 * One-call convenience wrapper for Node.js / SSR environments.
 *
 * @example
 * // Express middleware
 * import { createServerI18n } from "./server";
 * const i18n = await createServerI18n({ localesDir: "./dist/locales", fallbackLocale: "en" });
 * app.use((req, res, next) => { req.i18n = i18n; next(); });
 *
 * @example
 * // Next.js Route Handler (app/api/translate/route.ts)
 * import { getSharedI18n } from "@/lib/i18n-server";
 * export async function GET(req: Request) {
 *   const i18n = await getSharedI18n();
 *   return Response.json({ text: i18n.t("es", "app.greeting") });
 * }
 */
export async function createServerI18n(
  options: ServerI18nOptions
): Promise<I18nInstance> {
  const localesDir = options.localesDir ?? "./dist/locales";
  return createI18n({
    ...options,
    loader: options.loader ?? nodePakLoader(localesDir),
  });
}

// Re-export core types so consumers can import from a single file.
export type { I18nInstance, PakLoader };
export { createI18n };
