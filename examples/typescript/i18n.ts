/**
 * i18n.ts — l10n4x isomorphic core
 *
 * This module is environment-agnostic. It receives a `PakLoader` function so
 * the caller controls how `.pak` files are fetched: via the Fetch API in
 * browsers, via `fs/promises` on Node.js, or via any custom transport (e.g.
 * in-memory cache, CDN pre-fetch, etc.).
 *
 * Usage:
 *   import { createI18n } from "./i18n";
 *   import { nodePakLoader } from "./server";   // or browserPakLoader from "./client"
 *
 *   const i18n = await createI18n({ loader: nodePakLoader("/path/to/locales") });
 *   console.log(i18n.t("en", "app.greeting"));
 */

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Function that loads a .pak file and returns its raw bytes. */
export type PakLoader = (locale: string) => Promise<Uint8Array>;

export interface I18nOptions {
  /** Inject the loader implementation (browser fetch, Node fs, custom, …). */
  loader: PakLoader;
  /** WASM binary URL or pre-fetched Response/ArrayBuffer (browser only). */
  wasmUrl?: string | URL | Request | BufferSource | WebAssembly.Module;
  /** 32-byte Ed25519 public key for .pak signature verification. Required. */
  verifyKey: Uint8Array;
  /** 32-byte AES key for optional L10E envelope decryption. */
  decryptKey?: Uint8Array;
  /** Locale to use when a translation is missing. Default: "en". */
  fallbackLocale?: string;
  /** Locales to eagerly load during init. Default: [fallbackLocale]. */
  preloadLocales?: string[];
}

export interface I18nInstance {
  /**
   * Translate a key for the given locale.
   * Falls back to `fallbackLocale` if the locale is not loaded.
   */
  t(locale: string, key: string, params?: Record<string, string>): string;
  /**
   * Load a locale on demand. Safe to call multiple times; subsequent calls
   * are no-ops if the locale is already loaded.
   */
  loadLocale(locale: string): Promise<void>;
  /** Release all WASM-side state and unload locales. */
  dispose(): void;
  /** Set of currently-loaded locale codes. */
  readonly loadedLocales: ReadonlySet<string>;
  /** Configured fallback locale. */
  readonly fallbackLocale: string;
}

// ---------------------------------------------------------------------------
// Factory
// ---------------------------------------------------------------------------

/**
 * Initialize l10n4x and return an I18nInstance.
 *
 * Dynamically imports `l10n4x-wasm` so this module can be loaded on Node.js
 * without bundler intervention (the consumer is responsible for ensuring the
 * WASM package is available in the Node module graph, e.g. via a .mjs shim or
 * next.config.js `serverExternalPackages`).
 */
export async function createI18n(options: I18nOptions): Promise<I18nInstance> {
  const fallbackLocale = options.fallbackLocale ?? "en";

  // Dynamic import keeps this file parseable in any environment.
  const wasm = await import("l10n4x-wasm");

  // `init` is optional when the WASM is already instantiated (e.g., in a
  // Next.js edge runtime that pre-instantiates). Guard accordingly.
  if (typeof wasm.default === "function") {
    await (wasm.default as (input?: unknown) => Promise<unknown>)(
      options.wasmUrl
    );
  }

  wasm.l10n4x_set_verify_key(options.verifyKey);
  if (options.decryptKey) {
    wasm.l10n4x_set_decrypt_key(options.decryptKey);
  }
  wasm.l10n4x_set_fallback_locale(fallbackLocale);

  const loadedLocales = new Set<string>();

  async function loadLocale(locale: string): Promise<void> {
    if (loadedLocales.has(locale)) return;
    const bytes = await options.loader(locale);
    wasm.l10n4x_load_pak_bytes(bytes, locale);
    loadedLocales.add(locale);
  }

  // Preload: always include the fallback.
  const toPreload = new Set([
    fallbackLocale,
    ...(options.preloadLocales ?? []),
  ]);
  await Promise.all([...toPreload].map(loadLocale));

  return {
    get loadedLocales(): ReadonlySet<string> {
      return loadedLocales;
    },

    fallbackLocale,

    async loadLocale(locale: string): Promise<void> {
      await loadLocale(locale);
    },

    t(
      locale: string,
      key: string,
      params?: Record<string, string>
    ): string {
      const normalized = (locale || fallbackLocale).toLowerCase().trim();
      // Trigger background load if missing (fire-and-forget).
      if (!loadedLocales.has(normalized)) {
        loadLocale(normalized).catch((err) =>
          console.warn(`l10n4x: background load failed for '${normalized}':`, err)
        );
      }

      if (!params || Object.keys(params).length === 0) {
        return wasm.l10n4x_translate(normalized, key);
      }

      const keys = Object.keys(params);
      const values = keys.map((k) => params[k]!);
      return wasm.l10n4x_translate_with_params(normalized, key, keys, values);
    },

    dispose(): void {
      wasm.l10n4x_clear();
      loadedLocales.clear();
    },
  };
}
