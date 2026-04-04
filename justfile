alias b := build
alias c := check
alias t := test
alias tr := test-review
alias ta := test-accept
alias te := test-e2e
alias r := run
alias f := format
alias l := lint
alias lf := lintfix
alias cov := test-cov


build:
    cargo build --release

build-debug:
    cargo build

test:
    cargo test -p tests --test suite
    cargo test -p tests --test lsp

test-infer:
    cargo test -p tests --test suite infer_tests

test-watch:
    cargo watch -x "test -p tests --test suite"

test-review:
    cargo insta review

test-accept:
    cargo insta accept --all

test-e2e:
    cargo test -p tests --test suite e2e

test-cov:
    cargo llvm-cov -p tests --test suite --test lsp --html --open

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

check: format-check test lint

perf-flamegraph:
    cargo build --profile flamegraph -p lisette
    sudo flamegraph -o flamegraph.svg -- ./target/flamegraph/lis check tests/e2e/src/main.lis

perf-samply:
    cargo build --profile flamegraph -p lisette
    samply record ./target/flamegraph/lis check tests/e2e/src/main.lis

fuzz-parse duration="300":
    cargo +nightly fuzz run parse --sanitizer address -- -max_total_time={{duration}} -rss_limit_mb=2048 -dict=fuzz/lisette.dict

fuzz-infer duration="300":
    cargo +nightly fuzz run infer --sanitizer address -- -max_total_time={{duration}} -rss_limit_mb=2048 -dict=fuzz/lisette.dict

check-stdlib-typedefs:
    cargo run -p lisette -- check crates/stdlib/typedefs/

regenerate-stdlib-typedefs version="":
    cd tools/bindgen && just build
    just build # make binary to run bindgen
    ./target/release/lis bindgen stdlib {{version}}
    ./target/release/lis format crates/stdlib/typedefs/
    just build # recompile compiler to embed updated typedefs
    ./target/release/lis check crates/stdlib/typedefs/
    just format
