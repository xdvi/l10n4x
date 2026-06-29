#!/usr/bin/env python3
import os
import sys

from l10n import RELEASES_URL, Translator

def examples_dir() -> str:
    return os.path.abspath(os.path.join(os.path.dirname(__file__), ".."))

def main() -> int:
    pak_dir = os.path.join(examples_dir(), "dist", "locales")
    if len(sys.argv) > 1:
        pak_dir = sys.argv[1]

    try:
        tr = Translator()
    except FileNotFoundError as exc:
        print(exc, file=sys.stderr)
        print(f"hint: extract release assets to examples/lib/ — see {RELEASES_URL}", file=sys.stderr)
        return 1
    except Exception as exc:
        print(f"Failed to initialize Translator: {exc}")
        return 1

    verify_hex = os.environ.get("L10N4X_VERIFY_PUBLIC_KEY")
    if not verify_hex:
        print(
            "L10N4X_VERIFY_PUBLIC_KEY is not set "
            "(64-char hex from l10n4x.config.json verifyPublicKey)."
        )
        return 1

    try:
        tr.set_verify_key(bytes.fromhex(verify_hex))
    except Exception as exc:
        print(f"Failed to set verify public key: {exc}")
        return 1

    enc_key = os.environ.get("L10N4X_ENCRYPT_KEY")
    if enc_key:
        try:
            if len(enc_key) != 32:
                raise ValueError("L10N4X_ENCRYPT_KEY must be exactly 32 bytes")
            tr.set_decrypt_key(enc_key.encode("latin1"))
        except Exception as exc:
            print(f"Failed to set decryption key: {exc}")
            return 1

    try:
        tr.set_fallback_locale("es")
        tr.load_pak_directory(pak_dir)
    except Exception as exc:
        print(f"Initialization failed: {exc}")
        return 1

    # Check loaded store state via FFI if possible
    es_welcome = tr.translate("es", "common.welcome")
    en_welcome = tr.translate_buffered("en", "common.welcome")
    en_greet = tr.translate("en", "common.greet", params={"name": "World"})

    print(f"[es] welcome: {es_welcome}")
    print(f"[en] welcome: {en_welcome}")
    print(f"[en] greet:   {en_greet}")

    assert "Bienvenido" in es_welcome, f"unexpected es welcome: {es_welcome!r}"
    assert "Welcome" in en_welcome, f"unexpected en welcome: {en_welcome!r}"
    assert "World" in en_greet, f"unexpected en greet: {en_greet!r}"

    tr.clear()
    return 0

if __name__ == "__main__":
    sys.exit(main())