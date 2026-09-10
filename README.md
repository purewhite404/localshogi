# 将棋AI

ブラウザ上で実行できる将棋AIです。
サーバとの通信はありません。速度はお使いの端末のスペックに依存します。

## 実行

```bash
cargo install wasm-bindgen-cli --version 0.2.128
./build_wasm.sh
python3 -m http.server 8000
```

ブラウザで <http://localhost:8000/> を開きます。

## 参考
これらコードはすべて生成AI及びコーディングエージェントによって作られています。
また、そのプロンプトの一部において、以下の記事内のソースコードを読み込ませ、参考にさせています。
- [【連載】評価関数を作ってみよう！その10](https://yaneuraou.yaneu.com/2020/12/02/make-evaluate-function-10/)