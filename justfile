export INSTA_YAML_BLOCK_STYLE := "1"

build:
    cargo build --release

build-debug:
    cargo build

grammar:
    npm --prefix editors/tree-sitter-lisette ci
    npm --prefix editors/tree-sitter-lisette run build

# `fast_*` tests: the compiler and language server as a library, without `lis` or Go.
test-fast:
    cargo test -p tests --locked --test fast_suite --test fast_lsp --test fast_release -- --format terse

# Unit tests inside each crate.
test-unit:
    cargo test --workspace --lib --bins --locked -- --format terse
    cargo test -p lisette --bins --locked --features self-upgrade -- --format terse

test-infer:
    cargo test -p tests --test fast_suite infer_tests

test-watch:
    cargo watch -x "test -p tests --test fast_suite"

snap-review:
    cargo insta review

snap-accept:
    cargo insta accept --all

# `cli_*` tests: `lis` CLI commands on small projects.
test-cli:
    cargo build -p lisette --locked
    cd bindgen && go build -o bin/bindgen .
    cargo test -p tests --locked \
        --test cli_language \
        --test cli_run \
        --test cli_cross \
        --test cli_library \
        --test cli_go_dependencies \
        --test cli_external_tests \
        --test cli_learn \
        --test cli_lsp \
        -- --format terse

# `go_*` tests: Lisette against the Go toolchain. Name tests to run only those, e.g. `just test-go go_emit`.
test-go +tests="go_emit go_rejections go_equivalents":
    cargo test --profile go-tests -p tests --locked {{prepend("--test ", tests)}} -- --nocapture

# `fuzz_*` tests: random changes to test programs. `seconds` limits the run.
test-fuzz seconds="600":
    FUZZ_SECONDS={{seconds}} cargo test --release -p tests --locked --test fuzz_emit -- --nocapture

test-cov:
    cargo llvm-cov -p tests --test fast_suite --test fast_lsp --html --open

snap-refresh:
    cargo insta test --force-update-snapshots

format:
    cargo fmt

format-check:
    cargo fmt -- --check

lint:
    cargo clippy --all-targets -- -D warnings
    cargo clippy --release --all-targets -- -D warnings
    cargo clippy -p lisette --all-targets --features self-upgrade -- -D warnings

lint-satellites:
    cd playground/wasm && cargo fmt -- --check && cargo clippy --all-targets -- -D warnings
    cd editors/zed && cargo fmt -- --check && cargo clippy --all-targets -- -D warnings
    cd fuzz && cargo fmt -- --check && cargo clippy --locked --all-targets -- -D warnings

lintfix:
    cargo clippy --fix --allow-dirty --allow-staged

run file:
    cargo run -p lisette -- {{file}}

# What the git hook runs before each commit.
pre-commit: format-check test-fast test-unit lint

perf-flamegraph:
    cargo build --profile flamegraph -p lisette
    sudo flamegraph -o flamegraph.svg -- ./target/flamegraph/lis check tests/cli_language/project

perf-samply:
    cargo build --profile flamegraph -p lisette
    samply record ./target/flamegraph/lis check tests/cli_language/project

loc:
    git fetch origin gh-pages:gh-pages
    rm -rf target/loc-chart && mkdir -p target/loc-chart
    git archive gh-pages loc | tar -x -C target/loc-chart
    open target/loc-chart/loc/index.html

check-fuzz-targets:
    cargo check --locked --manifest-path fuzz/Cargo.toml

fuzz-parse duration="300":
    cargo +nightly fuzz run --sanitizer address parse fuzz/corpus/parse fuzz/seed_corpus -- -max_total_time={{duration}} -rss_limit_mb=2048 -dict=fuzz/lisette.dict

fuzz-infer duration="300":
    cargo +nightly fuzz run --sanitizer address infer fuzz/corpus/infer fuzz/seed_corpus -- -max_total_time={{duration}} -rss_limit_mb=2048 -dict=fuzz/lisette.dict

fuzz-scan-imports duration="300":
    cargo +nightly fuzz run --sanitizer address scan_imports fuzz/corpus/scan_imports fuzz/seed_corpus -- -max_total_time={{duration}} -rss_limit_mb=2048 -dict=fuzz/lisette.dict

_supported-targets := "linux/amd64,linux/arm64,darwin/amd64,darwin/arm64,windows/amd64,windows/arm64"
_pinned-go-toolchain := "go" + `cat go-toolchain`

generate-stdlib-typedefs version targets=_supported-targets:
    cd bindgen && GOTOOLCHAIN={{_pinned-go-toolchain}} just build
    just build # make binary to run bindgen
    BINDGEN_TARGETS={{targets}} GOTOOLCHAIN={{_pinned-go-toolchain}} ./target/release/lis bindgen stdlib {{version}}
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
    cd bindgen && GOTOOLCHAIN={{_pinned-go-toolchain}} just build
    cargo build --profile bindgen
    ./target/bindgen/lis check crates/stdlib/typedefs/
    BINDGEN_TARGETS={{_supported-targets}} GOTOOLCHAIN={{_pinned-go-toolchain}} ./target/bindgen/lis bindgen stdlib "$(just _stdlib-typedef-version)"
    ./target/bindgen/lis format crates/stdlib/typedefs/
    just format
    git diff --exit-code crates/stdlib/

# Build the playground and write output to site/public/play/ (served at lisette.run/play)
rebuild-playground:
    cd playground && npm install && npm run build:wasm && npm run build

# Prepare a Zed extension release: bump version, pin grammar to HEAD (also in the Helix config and the quickstart), write the PR body.
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
    perl -i -pe "s/^(\s*rev = )\"[0-9a-f]{40}\"/\$1\"$head\"/" editors/helix/README.md site/src/content/docs/intro/quickstart.mdx

    mkdir -p target
    {
      echo "Bumps the Zed extension to $new and advances the bundled tree-sitter grammar pin."
      echo; echo "Grammar-related commits since the last rev:"; echo
      echo '```'; git log --oneline "$pinned..$head" -- $paths; echo '```'
    } > target/zed-release-body.md

    echo "Bumped to $new (rev ${head:0:8}). PR body at target/zed-release-body.md; review, commit editors/zed, editors/helix, and the quickstart, open PR (see README)."

# release.yml is autogenerated by cargo-dist; ratchet must not modify it
_ratchet cmd:
    find .github/workflows .github/actions -name '*.yml' -not -name release.yml -exec ratchet {{cmd}} {} +

ci-pin: (_ratchet "pin")

ci-update: (_ratchet "update")

ci-upgrade: (_ratchet "upgrade")

ci-lint: (_ratchet "lint")
