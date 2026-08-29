#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Prints two `key=value` lines for CI to gate jobs on: `code` for anything outside
# `site/`, `site` for the sources the docs site build reads. Diagnostics go to stderr
# so the caller can redirect stdout straight into `$GITHUB_OUTPUT`.

EVENT=${EVENT:-pull_request}
BASE=${BASE:-main}

everything() {
    echo "code=true"
    echo "site=true"
    exit 0
}

# A push to main has no base to diff against, so it rebuilds everything.
[[ $EVENT != pull_request ]] && everything

# The CI checkout is shallow and fetches only the merge ref, so it leaves no
# remote-tracking ref for the base. Fetch one commit and read FETCH_HEAD instead. A
# full local clone already has the ref, and fetching shallow there would truncate it.
if git rev-parse --verify -q "origin/$BASE" > /dev/null; then
    base="origin/$BASE"
else
    git fetch --depth=1 origin "$BASE"
    base=FETCH_HEAD
fi

changed=$(git diff --name-only "$base..HEAD")

# An empty diff is unexpected, so run everything rather than nothing.
[[ -z $changed ]] && everything

echo "Changed paths:" >&2
echo "$changed" >&2

site_sources='^(site/|CHANGELOG\.md$|crates/cli/reference/|crates/cli/src/handlers/learn/|crates/stdlib/prelude\.d\.lis$|editors/vscode/syntaxes/lisette\.tmLanguage\.json$|\.github/workflows/ci\.yml$|scripts/classify-changes\.sh$)'

code=false
site=false
grep -qvE '^site/' <<< "$changed" && code=true
grep -qE "$site_sources" <<< "$changed" && site=true

echo "code=$code"
echo "site=$site"
