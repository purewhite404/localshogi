#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
rm -rf pkg
wasm-bindgen \
  target/wasm32-unknown-unknown/release/shogi_wasm.wasm \
  --target no-modules \
  --out-dir pkg

echo "Wasm package generated in ./pkg"
