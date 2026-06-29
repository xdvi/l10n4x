#!/usr/bin/env bash
# Smoke-test all l10n4x examples (dev/CI). Requires Rust toolchain in PATH.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
LIB_DIR="$ROOT/examples/lib"
PAK_DIR="$ROOT/examples/dist/locales"
# WARNING: This static seed is for test/CI verification only. Never use this in production.
SIGNING_SEED="$(printf '%0.s1' {1..32})"

export L10N4X_SIGNING_KEY="$SIGNING_SEED"

echo "==> Building l10n4c and l10n4x CLI"
cargo build --release -p l10n4c -p l10n4x-toolkit --manifest-path "$ROOT/Cargo.toml"

mkdir -p "$LIB_DIR"
cp "$ROOT/target/release/libl10n4c.so" "$LIB_DIR/"
cp "$ROOT/target/release/l10n4x" "$LIB_DIR/"
cp "$ROOT/packages/ffi/l10n4c.h" "$LIB_DIR/"

echo "==> Preparing locale fixtures"
mkdir -p "$ROOT/locales/en" "$ROOT/locales/es"
cat > "$ROOT/locales/en/common.json" <<'JSON'
{"welcome": "Welcome!", "greet": "Hello, {name}!"}
JSON
cat > "$ROOT/locales/es/common.json" <<'JSON'
{"welcome": "¡Bienvenido!", "greet": "¡Hola, {name}!"}
JSON

echo "==> Building signed .pak files"
(cd "$ROOT" && "$LIB_DIR/l10n4x" build)

VERIFY_HEX="$(python3 -c "import json; print(json.load(open('$ROOT/l10n4x.config.json'))['verifyPublicKey'])")"
export L10N4X_VERIFY_PUBLIC_KEY="$VERIFY_HEX"

pass=0
fail=0

run_check() {
  local name="$1"
  shift
  echo ""
  echo "==> $name"
  if "$@"; then
    echo "OK: $name"
    pass=$((pass + 1))
  else
    echo "FAIL: $name"
    fail=$((fail + 1))
  fi
}

run_check "Go example" bash -c "cd '$ROOT/examples/go' && go run ."
run_check "Python example" bash -c "cd '$ROOT/examples/python' && python3 main.py"
run_check "C# example" bash -c "cd '$ROOT/examples/csharp' && dotnet run --nologo -v q"

mkdir -p "$ROOT/examples/flutter/assets/locales"
cp "$PAK_DIR"/*.pak "$ROOT/examples/flutter/assets/locales/"

FLUTTER_DEFINE="--dart-define=L10N4X_VERIFY_PUBLIC_KEY=$VERIFY_HEX"
export L10N4X_LIB_DIR="$LIB_DIR"

if command -v flutter &>/dev/null; then
  run_check "Flutter analyze" bash -c "cd '$ROOT/examples/flutter' && flutter analyze"
  run_check "Flutter FFI test" bash -c "cd '$ROOT/examples/flutter' && flutter test $FLUTTER_DEFINE"
else
  echo ""
  echo "==> Skipping Flutter checks (flutter not in PATH)"
fi

echo ""
echo "Results: $pass passed, $fail failed"
if [[ "$fail" -gt 0 ]]; then
  exit 1
fi