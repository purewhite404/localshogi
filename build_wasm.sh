#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
wasm-pack build --release --target no-modules --out-dir www/pkg --no-typescript

echo "Wasm package generated in ./www/pkg"
