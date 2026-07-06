/**
 * client.ts — Browser entry point for l10n4x
 *
 * Uses the Fetch API to download .lpk files. Works in any browser environment
 * or bundler (Vite, webpack, Rollup, etc.).
 *
 * Example (in your app's bootstrap):
 *
 *   import { browserLpkLoader, createBrowserI18n } from "./client";
 *   const i18n = await createBrowserI18n({
 *     localesBaseUrl: "/locales",
 *     fallbackLocale: "en",
 *   });
 *   document.getElementById("greeting")!.textContent = i18n.t("es", "app.greeting");
 */

import { createI18n, type I18nInstance, type I18nOptions, type LpkLoader } from "./i18n";

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/**
 * Build a LpkLoader backed by the browser's Fetch API.
 *
 * @param baseUrl  URL prefix where .lpk files live (e.g. "/locales" or
 *                 "https://cdn.example.com/locales"). No trailing slash.
 */
export function browserLpkLoader(baseUrl: string): LpkLoader {
  return async (locale: string): Promise<Uint8Array> => {
    const url = `${baseUrl}/${locale}.lpk`;
    const res = await fetch(url);
    if (!res.ok) {
      throw new Error(
        `l10n4x: fetch failed for locale '${locale}': HTTP ${res.status} ${res.statusText} (${url})`
      );
    }
    return new Uint8Array(await res.arrayBuffer());
  };
}

// ---------------------------------------------------------------------------
// Convenience factory
// ---------------------------------------------------------------------------

export interface BrowserI18nOptions
  extends Omit<I18nOptions, "loader"> {
  /**
   * Base URL for .lpk files.
   * @default "/locales"
   */
  localesBaseUrl?: string;
  /** Custom loader (overrides localesBaseUrl). */
  loader?: LpkLoader;
}

/**
 * One-call convenience wrapper for browser environments.
 *
 * @example
 * const i18n = await createBrowserI18n({ localesBaseUrl: "/dist/locales" });
 * i18n.t("fr", "app.title");
 */
export async function createBrowserI18n(
  options: BrowserI18nOptions
): Promise<I18nInstance> {
  const baseUrl = options.localesBaseUrl ?? "/locales";
  return createI18n({
    ...options,
    loader: options.loader ?? browserLpkLoader(baseUrl),
  });
}

// Re-export core types so consumers can import from a single file.
export type { I18nInstance, LpkLoader };
export { createI18n };
