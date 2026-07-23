#!/usr/bin/env bash

set -euo pipefail
cd "$(dirname "$0")/.."

out=${1:?usage: scripts/loc-backfill.sh <output-data.js>}

workdir=$(mktemp -d)
trap 'rm -rf "$workdir"' EXIT

entries="$workdir/entries.ndjson"
: > "$entries"

for sha in $(git log --first-parent --reverse main --date=format:'%G-%V %Y-%m' --format='%H %cd' |
  awk '
    { order[NR] = $1; if (!($2 in week_first)) week_first[$2] = $1; month_last[$3] = $1 }
    END {
      for (w in week_first) keep[week_first[w]]
      for (m in month_last) keep[month_last[m]]
      for (i = 1; i <= NR; i++) if (order[i] in keep) print order[i]
    }'); do
  tree="$workdir/tree"
  mkdir "$tree"
  git archive "$sha" | tar -x -C "$tree"

  rust=$(cd "$tree" && cargo warloc --by-file -o json | jq '
    [.files | to_entries[]
     | select(.key | startswith("./crates/"))
     | {crate: (.key | split("/")[2]), code: .value.main.code}]
    | group_by(.crate)
    | map({name: .[0].crate, unit: "lines", value: (map(.code) | add)})
    | sort_by(-.value)')

  go=$(cd "$tree" && for dir in bindgen prelude; do
    [ -d "$dir" ] || continue
    tokei -t Go -e '*_test.go' -e tests -o json "$dir" |
      jq --arg name "$dir" '{name: $name, unit: "lines", value: .Go.code}'
  done | jq -s .)

  jq -n \
    --arg id "$sha" \
    --arg message "$(git show -s --format=%s "$sha")" \
    --arg timestamp "$(git show -s --format=%cI "$sha")" \
    --argjson date "$(($(git show -s --format=%ct "$sha") * 1000))" \
    --argjson benches "$(jq -n --argjson rust "$rust" --argjson go "$go" \
      '[{name: "total", unit: "lines", value: (($rust + $go) | map(.value) | add)}] + $rust + $go')" \
    '{commit: {id: $id, message: $message, timestamp: $timestamp,
       url: ("https://github.com/ivov/lisette/commit/" + $id)},
      date: $date, tool: "customSmallerIsBetter", benches: $benches}' >> "$entries"

  rm -rf "$tree"
  echo "counted $sha" >&2
done

{
  printf 'window.BENCHMARK_DATA = '
  jq -n --argjson entries "$(jq -s . "$entries")" \
    '{lastUpdate: 0, repoUrl: "https://github.com/ivov/lisette",
      entries: {"production-loc": $entries}}'
} > "$out"
