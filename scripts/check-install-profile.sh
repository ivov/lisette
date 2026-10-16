#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# `cargo publish --dry-run` verifies with the `dev` profile, so install the
# packaged crate to test the install profile.

MANIFEST=crates/cli/Cargo.toml
KEYS=(lto codegen-units strip)

scripts/add-install-profile.sh
trap 'git checkout -- "$MANIFEST"' EXIT

METADATA=$(cargo metadata --format-version 1 --no-deps)
VERSION=$(jq -r '.packages[] | select(.name == "lisette") | .version' <<< "$METADATA")
cargo package --workspace --exclude tests --allow-dirty --no-verify
CRATE="target/package/lisette-$VERSION.crate"

section() {
  awk -v header="$1" '/^\[/ { in_section = ($0 == header); next } in_section'
}
DIST=$(section "[profile.dist]" < Cargo.toml)
PACKAGED=$(tar -xzOf "$CRATE" "lisette-$VERSION/Cargo.toml" | section "[profile.release]")
for key in "${KEYS[@]}"; do
  EXPECTED=$(grep -E "^$key = " <<< "$DIST")
  grep -qxF "$EXPECTED" <<< "$PACKAGED" || { echo "Packaged manifest lacks \`$EXPECTED\` in [profile.release]:"; echo "$PACKAGED"; exit 1; }
done
echo "Packaged [profile.release]:"
echo "$PACKAGED"

# This version of the internal crates is not on crates.io yet.
PATCHES=()
while IFS=$'\t' read -r name dir; do
  PATCHES+=(--config "patch.crates-io.$name.path=\"$dir\"")
done < <(jq -r '.packages[] | select(.name != "lisette" and .publish == null) | [.name, (.manifest_path | rtrimstr("/Cargo.toml"))] | @tsv' <<< "$METADATA")

WORK=$(mktemp -d)
tar -xzf "$CRATE" -C "$WORK"
case "$(uname)" in
  Linux) MEASURE=(/usr/bin/time -v) ;;
  Darwin) MEASURE=(/usr/bin/time -l) ;;
  *) MEASURE=() ;;
esac
"${MEASURE[@]}" cargo install --path "$WORK/lisette-$VERSION" --root "$WORK/root" --target-dir "$WORK/target" "${PATCHES[@]}"

"$WORK/root/bin/lis" version
"$WORK/root/bin/lis" check tests/e2e_smoke_project
