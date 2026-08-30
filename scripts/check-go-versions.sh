#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

EXPECTED=$(cat go-toolchain | cut -d. -f1,2)
PRELUDE=$(grep '^go ' prelude/go.mod | awk '{print $2}')
BINDGEN=$(grep '^go ' bindgen/go.mod | awk '{print $2}' | cut -d. -f1,2)
EMBEDDED=$(cat bindgen/internal/cli/metadata/go-toolchain | tr -d '[:space:]')
CLI=$(cat crates/cli/go-toolchain | tr -d '[:space:]')
[ "$EXPECTED" = "$PRELUDE" ] || { echo "Go version mismatch: go-toolchain has $EXPECTED but prelude/go.mod has $PRELUDE"; exit 1; }
[ "$EXPECTED" = "$BINDGEN" ] || { echo "Go version mismatch: go-toolchain has $EXPECTED but bindgen/go.mod has $BINDGEN"; exit 1; }
[ "$(cat go-toolchain | tr -d '[:space:]')" = "$EMBEDDED" ] || { echo "Go version mismatch: go-toolchain has $(cat go-toolchain) but bindgen metadata has $EMBEDDED"; exit 1; }
[ "$(cat go-toolchain | tr -d '[:space:]')" = "$CLI" ] || { echo "Go version mismatch: go-toolchain has $(cat go-toolchain) but crates/cli/go-toolchain has $CLI"; exit 1; }
