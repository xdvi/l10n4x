/**
 * demo-browser.ts — Browser demo for l10n4x
 *
 * Bundle with Vite (or any bundler that handles .wasm imports):
 *   cd examples/typescript
 *   npx vite --open
 *
 * The bundler must copy .lpk files to the public/locales directory, or you
 * can configure vite.config.ts to serve them from a static path.
 */

import { createBrowserI18n } from "./client";

let currentLocale = "en";
let i18n: Awaited<ReturnType<typeof createBrowserI18n>>;

/**
 * In a real app, the verify key hex would be injected at build time (e.g.
 * via Vite's `define`, webpack DefinePlugin, or an env variable).
 * This demo reads it from a global or defaults to a placeholder.
 */
function getVerifyKeyHex(): string {
  // Set via Vite define / env:
  // import.meta.env.VITE_L10N4X_VERIFY_PUBLIC_KEY
  return (
    (globalThis as unknown as Record<string, string>).__L10N4X_VERIFY_KEY__ ??
    "0000000000000000000000000000000000000000000000000000000000000000"
  );
}

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function render() {
  const greetingEl = document.getElementById("greeting")!;
  const welcomeEl = document.getElementById("welcome")!;

  greetingEl.textContent = i18n.t(currentLocale, "common.welcome");
  welcomeEl.textContent = i18n.t(currentLocale, "common.greet", {
    name: "World",
  });
}

async function main() {
  i18n = await createBrowserI18n({
    localesBaseUrl: "/locales",
    fallbackLocale: "en",
    preloadLocales: ["en", "es"],
    verifyKey: hexToBytes(getVerifyKeyHex()),
  });

  render();

  document.getElementById("switch-es")!.addEventListener("click", async () => {
    await i18n.loadLocale("es");
    currentLocale = "es";
    render();
  });

  document.getElementById("switch-en")!.addEventListener("click", () => {
    currentLocale = "en";
    render();
  });
}

main().catch(console.error);
