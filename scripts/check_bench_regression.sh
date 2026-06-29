#!/usr/bin/env bash
# Compare criterion benches against a saved baseline; exit non-zero on >threshold% regression.
set -euo pipefail

THRESHOLD="${1:-5}"
BASELINE="${2:-main}"
BENCHES="${BENCHES:-translate_alloc_cache_hit swap_store_reload load_raw_bytes_reload}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

mkdir -p target/criterion

echo "Running benches with baseline '${BASELINE}' (threshold ${THRESHOLD}%)..."
REGRESSED=0

for bench in $BENCHES; do
  echo "--- ${bench} ---"
  if ! OUTPUT=$(cargo bench -p l10n4x-core --bench lookup -- "$bench" --baseline "$BASELINE" 2>&1); then
    echo "$OUTPUT"
    echo "Bench run failed for ${bench}"
    exit 1
  fi
  echo "$OUTPUT"
  if echo "$OUTPUT" | grep -E 'Performance has regressed'; then
    PCT=$(echo "$OUTPUT" | grep -oE 'change: \+[0-9]+\.[0-9]+%' | head -1 | grep -oE '[0-9]+\.[0-9]+' || echo "999")
    if awk -v p="$PCT" -v t="$THRESHOLD" 'BEGIN { exit (p > t) ? 0 : 1 }'; then
      echo "REGRESSION: ${bench} +${PCT}% (> ${THRESHOLD}%)"
      REGRESSED=1
    fi
  fi
done

if [[ "$REGRESSED" -ne 0 ]]; then
  echo "Benchmark regression check failed."
  exit 1
fi

echo "Benchmark regression check passed."