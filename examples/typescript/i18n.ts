/**
 * i18n.ts — l10n4x isomorphic core
 *
 * This module is environment-agnostic. It receives a `LpkLoader` function so
 * the caller controls how `.lpk` files are fetched: via the Fetch API in
 * browsers, via `fs/promises` on Node.js, or via any custom transport (e.g.
 * in-memory cache, CDN pre-fetch, etc.).
 *
 * Usage:
 *   import { createI18n } from "./i18n";
 *   import { nodeLpkLoader } from "./server";   // or browserLpkLoader from "./client"
 *
 *   const i18n = await createI18n({ loader: nodeLpkLoader("/path/to/locales") });
 *   console.log(i18n.t("en", "app.greeting"));
 */

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** FNV-1a 64‑bit hash — must match `l10n4x_core::binary_format::fnv1a_64`. */
const FNV1A_OFFSET = 0xcbf29ce484222325n;
const FNV1A_PRIME = 0x100000001b3n;
const MASK_64 = 0xFFFFFFFFFFFFFFFFn;

const encoder = new TextEncoder();

function fnv1a_64(data: Uint8Array): bigint {
  let hash = FNV1A_OFFSET;
  for (const b of data) {
    hash ^= BigInt(b);
    hash = (hash * FNV1A_PRIME) & MASK_64;
  }
  return hash;
}

/** Function that loads a .lpk file and returns its raw bytes. */
export type LpkLoader = (locale: string) => Promise<Uint8Array>;

export interface I18nOptions {
  /** Inject the loader implementation (browser fetch, Node fs, custom, …). */
  loader: LpkLoader;
  /** WASM binary URL or pre-fetched Response/ArrayBuffer (browser only). */
  wasmUrl?: string | URL | Request | BufferSource | WebAssembly.Module;
  /** 32-byte Ed25519 public key for .lpk signature verification. Required. */
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
    wasm.l10n4x_load_lpk_bytes(bytes, locale);
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

      const keyHash = fnv1a_64(encoder.encode(key));

      if (!params || Object.keys(params).length === 0) {
        return wasm.l10n4x_translate(normalized, keyHash);
      }

      const keys = Object.keys(params);
      const values = keys.map((k) => params[k]!);
      return wasm.l10n4x_translate_with_params(normalized, keyHash, keys, values);
    },

    dispose(): void {
      wasm.l10n4x_clear();
      loadedLocales.clear();
    },
  };
}
