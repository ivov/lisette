#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# go-toolchain: The Go release lis works with. Sets the toolchain lis runs,
# the stdlib the typedefs expose, what CI installs, and the `go` directive
# of app and script builds. Raising it is routine.
# go-language: The Go version emitted libraries and the prelude declare in
# their `go.mod`, so the minimum Go their consumers can build with. Raising
# this is a breaking change for those consumers.

TOOLCHAIN=$(cat go-toolchain | tr -d '[:space:]')
EXPECTED=$(echo "$TOOLCHAIN" | cut -d. -f1,2)
LANGUAGE=$(cat go-language | tr -d '[:space:]')
CLI_LANGUAGE=$(cat crates/cli/go-language | tr -d '[:space:]')
PRELUDE=$(grep '^go ' prelude/go.mod | awk '{print $2}')
BINDGEN=$(grep '^go ' bindgen/go.mod | awk '{print $2}' | cut -d. -f1,2)
EMBEDDED=$(cat bindgen/internal/cli/metadata/go-toolchain | tr -d '[:space:]')
CLI=$(cat crates/cli/go-toolchain | tr -d '[:space:]')
[ "$LANGUAGE" = "$PRELUDE" ] || { echo "Go version mismatch: go-language has $LANGUAGE but prelude/go.mod has $PRELUDE"; exit 1; }
[ "$LANGUAGE" = "$CLI_LANGUAGE" ] || { echo "Go version mismatch: go-language has $LANGUAGE but crates/cli/go-language has $CLI_LANGUAGE"; exit 1; }
[ "$EXPECTED" = "$BINDGEN" ] || { echo "Go version mismatch: go-toolchain has $EXPECTED but bindgen/go.mod has $BINDGEN"; exit 1; }
[ "$TOOLCHAIN" = "$EMBEDDED" ] || { echo "Go version mismatch: go-toolchain has $TOOLCHAIN but bindgen metadata has $EMBEDDED"; exit 1; }
[ "$TOOLCHAIN" = "$CLI" ] || { echo "Go version mismatch: go-toolchain has $TOOLCHAIN but crates/cli/go-toolchain has $CLI"; exit 1; }
BADGE=$(grep -o 'Go-[0-9.]*-' README.md | head -1 | sed 's/^Go-//; s/-$//' || true)
QUICKSTART=$(grep -o 'Install Go [0-9.]*' site/src/content/docs/intro/quickstart.mdx | head -1 | awk '{print $3}' || true)
[ -n "$BADGE" ] || { echo "No Go-<version> badge found in README.md"; exit 1; }
[ -n "$QUICKSTART" ] || { echo "No 'Install Go <version>' line found in site/src/content/docs/intro/quickstart.mdx"; exit 1; }
[ "$TOOLCHAIN" = "$BADGE" ] || { echo "Go version mismatch: go-toolchain has $TOOLCHAIN but the README badge has $BADGE"; exit 1; }
[ "$EXPECTED" = "$QUICKSTART" ] || { echo "Go version mismatch: go-toolchain has $EXPECTED but the quickstart says $QUICKSTART"; exit 1; }
[ "$(printf '%s\n%s\n' "$LANGUAGE" "$TOOLCHAIN" | sort -V | head -1)" = "$LANGUAGE" ] || { echo "go-language ($LANGUAGE) must not exceed go-toolchain ($TOOLCHAIN)"; exit 1; }
