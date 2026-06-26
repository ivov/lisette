alias b := build
alias c := check
alias t := test
alias tu := test-unit
alias tr := test-review
alias ta := test-accept
alias te := test-e2e-smoke
alias r := run
alias f := format
alias l := lint
alias lf := lintfix
alias cov := test-cov

export INSTA_YAML_BLOCK_STYLE := "1"

build:
    cargo build --release

build-debug:
    cargo build

grammar:
    npm --prefix editors/tree-sitter-lisette ci
    npm --prefix editors/tree-sitter-lisette run build

test:
    cargo test -p tests --test suite
    cargo test -p lisette-lsp --test lsp
    cargo test -p tests --test manifest_pins

test-unit:
    cargo test --workspace --lib --bins

test-infer:
    cargo test -p tests --test suite infer_tests

test-watch:
    cargo watch -x "test -p tests --test suite"

test-review:
    cargo insta review

test-accept:
    cargo insta accept --all

test-e2e-smoke:
    cargo build -p lisette
    cargo test -p tests --test e2e_smoke

test-e2e-suite:
    cargo test -p tests --test e2e_suite -- --nocapture

test-embed-diff:
    cargo test -p tests --test embed_diff -- --nocapture

test-cov:
    cargo llvm-cov -p tests -p lisette-lsp --test suite --test lsp --html --open

test-refresh-snapshots:
    cargo insta test --force-update-snapshots

format:
    cargo fmt

format-check:
    cargo fmt -- --check

lint:
    cargo clippy --all-targets -- -D warnings

lintfix:
    cargo clippy --fix --allow-dirty --allow-staged

run file:
    cargo run -p lisette -- {{file}}

check: format-check test test-unit lint

perf-flamegraph:
    cargo build --profile flamegraph -p lisette
    sudo flamegraph -o flamegraph.svg -- ./target/flamegraph/lis check tests/e2e_smoke_project/src/main.lis

perf-samply:
    cargo build --profile flamegraph -p lisette
    samply record ./target/flamegraph/lis check tests/e2e_smoke_project/src/main.lis

fuzz-parse duration="300":
    cargo +nightly fuzz run parse --sanitizer address -- -max_total_time={{duration}} -rss_limit_mb=2048 -dict=fuzz/lisette.dict

fuzz-infer duration="300":
    cargo +nightly fuzz run infer --sanitizer address -- -max_total_time={{duration}} -rss_limit_mb=2048 -dict=fuzz/lisette.dict

_supported-targets := "linux/amd64,linux/arm64,darwin/amd64,darwin/arm64,windows/amd64"

generate-stdlib-typedefs version targets=_supported-targets:
    cd bindgen && just build
    just build # make binary to run bindgen
    BINDGEN_TARGETS={{targets}} ./target/release/lis bindgen stdlib {{version}}
    ./target/release/lis format crates/stdlib/typedefs/
    just build # recompile compiler to embed updated typedefs
    ./target/release/lis check crates/stdlib/typedefs/
    just format

commit-stdlib-typedefs version:
    git add crates/stdlib/
    LEFTHOOK=0 git commit -m "chore: bump stdlib typedefs to v{{version}}"

_stdlib-typedef-version:
    @grep '// Lisette:' crates/stdlib/typedefs/fmt.d.lis | awk '{print $3}'

check-stdlib-drift:
    cd bindgen && just build
    cargo build --profile bindgen
    ./target/bindgen/lis check crates/stdlib/typedefs/
    BINDGEN_TARGETS={{_supported-targets}} ./target/bindgen/lis bindgen stdlib "$(just _stdlib-typedef-version)"
    ./target/bindgen/lis format crates/stdlib/typedefs/
    just format
    git diff --exit-code crates/stdlib/

# Build the playground and write output to docs/play/ (served at lisette.run/play)
rebuild-playground:
    cd playground && npm install && npm run build:wasm && npm run build

# Prepare a Zed extension release: bump version, pin grammar to HEAD, write the PR body.
zed-release bump="patch":
    #!/usr/bin/env bash
    set -euo pipefail
    ext=editors/zed/extension.toml
    paths="editors/tree-sitter-lisette editors/zed/languages"
    pinned=$(awk -F'"' '/^rev = /{print $2; exit}' "$ext")
    head=$(git rev-parse HEAD)
    [ -z "$(git status --porcelain -- $paths)" ] || { echo "Commit grammar/query changes before releasing; the rev is pinned to HEAD."; exit 1; }
    git diff --quiet "$pinned" "$head" -- $paths && { echo "Nothing to release since ${pinned:0:8}."; exit 0; }

    IFS=. read -r x y z < <(awk -F'"' '/^version = /{print $2; exit}' "$ext")
    case "{{bump}}" in
      patch) new="$x.$y.$((z + 1))" ;; minor) new="$x.$((y + 1)).0" ;; major) new="$((x + 1)).0.0" ;;
      *) [[ "{{bump}}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "Invalid bump: {{bump}} (use patch|minor|major|X.Y.Z)."; exit 1; }; new="{{bump}}" ;;
    esac

    perl -i -pe "s/^(version = )\"[^\"]*\"/\$1\"$new\"/" "$ext" editors/zed/Cargo.toml
    perl -i -pe "s/^(rev = )\"[^\"]*\"/\$1\"$head\"/" "$ext"
    perl -0i -pe "s/(zed_lisette\"\nversion = )\"[^\"]*\"/\$1\"$new\"/" editors/zed/Cargo.lock

    mkdir -p target
    {
      echo "Bumps the Zed extension to $new and advances the bundled tree-sitter grammar pin."
      echo; echo "Grammar-related commits since the last rev:"; echo
      echo '```'; git log --oneline "$pinned..$head" -- $paths; echo '```'
    } > target/zed-release-body.md

    echo "Bumped to $new (rev ${head:0:8}). PR body at target/zed-release-body.md; review, commit editors/zed, open PR (see README)."

# release.yml is autogenerated by cargo-dist; ratchet must not modify it
_ratchet cmd:
    find .github/workflows .github/actions -name '*.yml' -not -name release.yml -exec ratchet {{cmd}} {} +

ci-pin: (_ratchet "pin")

ci-update: (_ratchet "update")

ci-upgrade: (_ratchet "upgrade")

ci-lint: (_ratchet "lint")
