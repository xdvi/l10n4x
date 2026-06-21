/**
 * demo-node.ts — Runnable demo for Node.js
 *
 * Run:
 *   L10N4X_VERIFY_PUBLIC_KEY=<hex> npx tsx demo-node.ts
 *
 * Reads locale files from ../dist/locales/ (adjust path as needed).
 * The verify key hex is extracted from l10n4x.config.json at build time.
 */

import { createServerI18n } from "./server";

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

async function main() {
  const verifyHex = process.env.L10N4X_VERIFY_PUBLIC_KEY;
  if (!verifyHex) {
    console.error(
      "Error: L10N4X_VERIFY_PUBLIC_KEY env var is required.\n" +
      "  Extract it from l10n4x.config.json or pass it from CI."
    );
    process.exit(1);
  }

  console.log("l10n4x — Node.js / SSR demo\n");

  const i18n = await createServerI18n({
    localesDir: "../dist/locales",
    fallbackLocale: "en",
    preloadLocales: ["en", "es"],
    verifyKey: hexToBytes(verifyHex),
  });

  console.log("Loaded locales:", [...i18n.loadedLocales]);
  console.log();

  // --- Simple translation ---
  console.log("[en] common.welcome →", i18n.t("en", "common.welcome"));
  console.log("[es] common.welcome →", i18n.t("es", "common.welcome"));
  console.log();

  // --- With parameters ---
  console.log(
    "[en] common.greet (name=Alice) →",
    i18n.t("en", "common.greet", { name: "Alice" })
  );
  console.log(
    "[es] common.greet (name=María) →",
    i18n.t("es", "common.greet", { name: "María" })
  );

  // --- Cleanup ---
  i18n.dispose();
  console.log("\nDone.");
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
