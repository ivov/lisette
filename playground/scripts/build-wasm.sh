#!/usr/bin/env bash
# Build the Lisette WASM module and copy output to public/wasm/
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

WASM_DIR="${PROJECT_ROOT}/wasm"
OUT_DIR="${PROJECT_ROOT}/public/wasm"

echo "▶ Building WASM module (this may take a while on first run)..."
cd "${WASM_DIR}"

# Ensure wasm target is installed
rustup target add wasm32-unknown-unknown 2>/dev/null || true

wasm-pack build \
  --target web \
  --out-dir "${OUT_DIR}" \
  --release \
  -- \
  --config "build.rustflags=['-C','opt-level=z']"

echo "✓ WASM module written to ${OUT_DIR}"
echo "  Files:"
ls -lh "${OUT_DIR}"/*.wasm "${OUT_DIR}"/*.js 2>/dev/null || true
