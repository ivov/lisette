#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Cargo does not publish workspace profiles, so `cargo install lisette` gets
# these `dist` settings only from the published member manifest.

MANIFEST=crates/cli/Cargo.toml
KEYS=(lto codegen-units strip)

[ -z "$(git status --porcelain)" ] || { echo "Working tree must be clean before adding the install profile"; git status --short; exit 1; }

DIST=$(awk '/^\[/ { in_dist = ($0 == "[profile.dist]"); next } in_dist' Cargo.toml)
PROFILE=$'\n[profile.release]'
for key in "${KEYS[@]}"; do
  LINE=$(grep -E "^$key = " <<< "$DIST" || true)
  [ -n "$LINE" ] || { echo "No \`$key\` in [profile.dist] of Cargo.toml"; exit 1; }
  PROFILE+=$'\n'"$LINE"
done
echo "$PROFILE" >> "$MANIFEST"

CHANGED=$(git status --porcelain)
[ "$CHANGED" = " M $MANIFEST" ] || { echo "Expected only $MANIFEST to change, got:"; echo "$CHANGED"; exit 1; }
echo "Added to $MANIFEST:$PROFILE"
