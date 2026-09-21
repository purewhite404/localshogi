# 将棋AI

Rust + WebAssembly で動く将棋 AI。すべてブラウザ内で完結し、サーバ通信はありません。
将棋のルール（合法手生成・打ち歩詰め・千日手・連続王手の千日手）はすべて Rust 側で
実装されており、JavaScript は描画と入力のみを担当します。

## 構成

| パス | 内容 |
|---|---|
| `src/`          | エンジン本体（盤面表現・合法手生成・探索・評価関数）。ネイティブ／wasm 共通 |
| `src/wasm_api.rs` | `wasm-bindgen` による JS 向けバインディング（`#[cfg(target_arch="wasm32")]`） |
| `src/bin/`       | 学習データ生成・学習・対局ハーネスなど、ネイティブ専用ツール群 |
| `www/`           | 静的サイト（`pkg/` はビルド生成物、Git 管理外） |
| `nets/`          | 量子化済み NNUE の重み（コミット対象） |

## 必要なもの

- Rust stable（`wasm32-unknown-unknown` ターゲット）
- wasm-pack: `cargo install wasm-pack`
- ブラウザ: Chrome 91+ / Firefox 89+ / Safari 16.4+（WebAssembly SIMD 必須）

## ビルドとローカル実行

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
./build_wasm.sh
python3 -m http.server 8000 --directory www
```

ブラウザで <http://localhost:8000/> を開きます。`www/pkg/` は Git 管理外なので、
クローン直後は必ず `./build_wasm.sh` を実行してください。

## テスト

```bash
cargo test --release                         # 単体テスト + perft（深さ1〜4）
cargo test --release -- --ignored             # perft 深さ5・6（数秒〜数分）
node tools/selfplay-check.mjs                 # 実際の wasm 出力を Node から叩く結合テスト
```

`cargo test` に含まれる perft（深さ1〜6：30 / 900 / 25,470 / 719,731 /
19,861,490 / 547,581,517）は、独立実装の `shunsai` crate と突き合わせて検証済みの
合法手生成の正しさのゲートです。合法手生成に手を入れたときは必ずここを通してから
先に進んでください。

## デプロイ

main / master への push で GitHub Actions がテスト → wasm ビルド → GitHub Pages への
公開を行います（`actions/deploy-pages` を使用。`www/pkg/` はコミットしません）。
リポジトリの Settings → Pages → Source を「GitHub Actions」に設定してください
（旧 `gh-pages` ブランチ配信からの変更点です）。

## 既知の制限

- 持将棋（入玉宣言・27点法）は未実装です。
- 詰将棋探索（df-pn 等）は未実装です。通常の探索（反復深化＋置換表＋静止探索）のみ。
- 読み筋（PV）表示は簡易的に USI 表記で行っています（同・上/引/寄 等の完全な棋譜表記は未対応）。
