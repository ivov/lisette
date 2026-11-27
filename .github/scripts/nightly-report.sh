#!/usr/bin/env bash
set -euo pipefail

limit=65000

body() {
  local report=$1 out=$2
  mkdir -p "$(dirname "$out")"
  if [ -s "$report" ]; then
    cp "$report" "$out"
  else
    printf 'The job failed without a report. See the run log.\n' > "$out"
  fi
}

append() {
  local out=$1 heading=$2 file=$3
  if [ $(( $(wc -c < "$out") + $(wc -c < "$file") + ${#heading} + 2 )) -gt "$limit" ]; then
    printf '\n%s\n\nThis section does not fit in the issue. See the run log.\n' "$heading" >> "$out"
  else
    printf '\n%s\n\n' "$heading" >> "$out"
    cat "$file" >> "$out"
  fi
}

crash() {
  local title=$1 target=$2 artifacts=$3 out=$4
  mkdir -p "$(dirname "$out")"
  if [ -n "$(ls -A "$artifacts" 2>/dev/null)" ]; then
    printf -- "- %s: crashed. The input is in the \`fuzz-crash-%s\` artifact.\n" "$title" "$target" > "$out"
  else
    printf -- "- %s: the job failed without a crash, probably because the \`%s\` target no longer builds.\n" "$title" "$target" > "$out"
  fi
}

join() {
  local dir=$1 out=$2 run=$3
  printf 'Failed jobs in nightly run [%s](%s).\n' "${run##*/}" "$run" > "$out"
  shopt -s nullglob
  local crashes=("$dir"/crashes-*.md)
  local has_crashes=${#crashes[@]} has_emitter=0 has_metamorphic=0
  [ -f "$dir/emitter.md" ] && has_emitter=1
  [ -f "$dir/metamorphic.md" ] && has_metamorphic=1
  if [ $((has_crashes + has_emitter + has_metamorphic)) -eq 0 ]; then
    printf '\nNo job wrote a report. See the run log.\n' >> "$out"
    return
  fi
  if [ $((has_crashes + has_emitter)) -gt 0 ]; then
    printf '\n## Fuzzing\n' >> "$out"
  fi
  if [ "$has_crashes" -gt 0 ]; then
    cat "${crashes[@]}" > "$dir/crashes.md"
    append "$out" '### Crashes' "$dir/crashes.md"
  fi
  if [ "$has_emitter" -eq 1 ]; then
    append "$out" '### Emitter' "$dir/emitter.md"
  fi
  if [ "$has_metamorphic" -eq 1 ]; then
    append "$out" '## Metamorphic testing' "$dir/metamorphic.md"
  fi
}

"$@"
