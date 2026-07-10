#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

rust=$(cargo warloc --by-file -o json | jq '
  [.files | to_entries[]
   | select(.key | startswith("./crates/"))
   | {crate: (.key | split("/")[2]), code: .value.main.code}]
  | group_by(.crate)
  | map({name: .[0].crate, unit: "lines", value: (map(.code) | add)})
  | sort_by(-.value)')

go=$(for dir in bindgen prelude; do
  tokei -t Go -e '*_test.go' -e tests -o json "$dir" |
    jq --arg name "$dir" '{name: $name, unit: "lines", value: .Go.code}'
done | jq -s .)

jq -n --argjson rust "$rust" --argjson go "$go" \
  '[{name: "total", unit: "lines", value: (($rust + $go) | map(.value) | add)}] + $rust + $go'
