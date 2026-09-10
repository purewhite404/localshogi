# 将棋AI

Rust/Wasm製の将棋AIです。探索はWeb Workerで実行します。

## WSLで実行

```bash
cargo install wasm-bindgen-cli --version 0.2.128
./build_wasm.sh
python3 -m http.server 8000
```

ブラウザで <http://localhost:8000/> を開きます。
