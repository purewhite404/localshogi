# 将棋AI

Rust + WebAssemblyで動作する将棋AIです。探索はブラウザ上のWeb Workerで実行します。

## WSLで実行

```bash
cargo install wasm-pack
./build_wasm.sh
python3 -m http.server 8000 --directory www
```

ブラウザで <http://localhost:8000/> を開きます。

GitHubへpushすると、GitHub Actionsが`www/`を`gh-pages`ブランチへデプロイします。
