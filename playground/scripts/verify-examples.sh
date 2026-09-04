#!/bin/sh
# Formats, checks and runs every example in examples/ with `lis`, or with the binary in $LIS.
set -u
LIS="${LIS:-lis}"
DIR="$(cd "$(dirname "$0")/.." && pwd)/examples"
WORK="$(mktemp -d)"
status=0
for file in "$DIR"/*.lis; do
  name=$(basename "$file")
  cp "$file" "$WORK/$name"
  echo "===== $name"
  "$LIS" format "$WORK/$name" >/dev/null 2>&1
  if diff "$file" "$WORK/$name"; then echo "format: unchanged"; else echo "format: CHANGED"; status=1; fi
  "$LIS" check "$WORK/$name" || status=1
  "$LIS" run "$WORK/$name" || status=1
done
rm -rf "$WORK"
exit $status
