#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

command -v wasm-pack >/dev/null || { echo "wasm-pack が必要です: cargo install wasm-pack" >&2; exit 1; }

rm -rf www/pkg

wasm-pack build \
  --release \
  --target web \
  --out-dir www/pkg \
  --out-name shogi_engine \
  --no-typescript \
  --no-pack

rm -f www/pkg/.gitignore www/pkg/README.md

ls -lh www/pkg/shogi_engine_bg.wasm
echo "Wasm package generated in ./www/pkg"
echo "ローカル確認: python3 -m http.server 8000 --directory www"
