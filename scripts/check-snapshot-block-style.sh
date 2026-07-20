#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

quoted_scalar='^[a-z_]+: "'
unescaped_newline='(^|[^\])(\\\\)*\\n'

scrunched=()
while IFS= read -r -d '' snapshot; do
    {
        IFS= read -r line && [[ $line == --- ]] || continue
        while IFS= read -r line && [[ $line != --- ]]; do
            if [[ $line =~ $quoted_scalar && $line =~ $unescaped_newline ]]; then
                scrunched+=("$snapshot")
                break
            fi
        done
    } < "$snapshot"
done < <(find tests crates -name '*.snap' -print0)

if ((${#scrunched[@]} > 0)); then
    printf '%s\n' "${scrunched[@]}" | sort
    printf '\n%d snapshot(s) were generated without INSTA_YAML_BLOCK_STYLE=1.\n' "${#scrunched[@]}"
    echo "Fix with: just test-refresh-snapshots"
    exit 1
fi
