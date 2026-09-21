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

## NNUE 評価関数と学習パイプライン

評価関数はプラガブルで、デフォルトは従来のハンドクラフト評価
（`src/eval/handcrafted.rs`）です。小型 NNUE（入力2,344次元 → 128×2 → 16 → 1、
量子化後 約600KB、特徴量は「絶対位置」と「持ち駒（サーモメータ型）」のみ——
king-relative 特徴は今回のスコープでは省略。理由は `src/eval/nnue.rs` の doc
コメントを参照）も実装・学習して同梱していますが、**今回このセッション内で生成
できた自己対局データ（約69,000局面、3000ノード/手、10スレッドで約13分）では、
304K パラメータのネットを鍛えるには量が全く足りず、`match_bin` によるハンドクラフト
評価との対局検証で19敗1勝と明確に負け越しました。** そのため現状はハンドクラフト
評価をデフォルトのまま出荷し、NNUE は UI の「評価関数」セレクタから選べる実験的
オプションとして残しています。学習パイプライン自体（データ生成・勾配計算は
数値微分で検証済み・量子化・強さ検証）は完成しているので、続きは主に「もっと多くの
自己対局データを生成して再学習する」だけです：

```bash
# 1. 自己対局で学習データを生成（各スレッドが独立に対局し、自分のシャードに書く）
cargo run --release --features native --bin gen -- \
  --threads 10 --games-per-thread 5000 --nodes-per-move 3000 \
  --out-dir data/it0

# 2. 学習（Adam、シグモイド交差エントロピー損失、置換表スコアと対局結果を
#    λ で混合。学習後に量子化して .bin を書き出し、再学習用に float の
#    チェックポイント .fnet も残す）
cargo run --release --features native --bin train -- \
  --data-dir data/it0 --out nets/current.bin --epochs 15 --lambda 1.0

# 3. 強さの検証（固定ノード数・先後入れ替えのペア対局・簡易 Elo 推定）
cargo run --release --features native --bin match_bin -- \
  -a nets/current.bin -b hc --games 400 --nodes 5000
```

新しいネットを差し替えたら `./build_wasm.sh` を再実行してください
（`src/wasm_api.rs` が `nets/current.bin` を `include_bytes!` で埋め込みます）。
アーキテクチャが変わっていた場合はヘッダのハッシュ不一致で読み込み時にエラーになり、
ハンドクラフト評価へ自動フォールバックします（`match_bin` で旧ネットに対して
有意に勝ち越していることを確認してから commit してください）。

## デプロイ

main / master への push で GitHub Actions がテスト → wasm ビルド → GitHub Pages への
公開を行います（`actions/deploy-pages` を使用。`www/pkg/` はコミットしません）。
リポジトリの Settings → Pages → Source を「GitHub Actions」に設定してください
（旧 `gh-pages` ブランチ配信からの変更点です）。

## 既知の制限

- 持将棋（入玉宣言・27点法）は未実装です。
- 詰将棋探索（df-pn 等）は未実装です。通常の探索（反復深化＋置換表＋静止探索）のみ。
- 読み筋（PV）表示は簡易的に USI 表記で行っています（同・上/引/寄 等の完全な棋譜表記は未対応）。
