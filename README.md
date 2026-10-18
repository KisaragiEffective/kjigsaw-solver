# kzigsaw-solver

## crates

* `kzigsaw-solver-core`: コア実装
* `kzigsaw-solver-parallelization`: 並列実装 - `rayon` などはここで
* `kzigsaw-solver-cli`: CLI
* `kzigsaw-solver-webapp`: core, parallelization を web-sys, js-sys と一緒に WebAssembly として同梱する

## CLI 実装のインターフェース

* `kzigsaw-solver-cli help`: ヘルプを表示。
* `kzigsaw-solver-cli detect-edge`: エッジを検出。
* `kzigsaw-solver-cli solve`: パズルを並び替える。
    * `--assume-all-pieces-are-same-shape`: 必須。将来の一般化に向けて意味のないフラグとして作成。
    * `--assume-all-pieces-are-rectangle`: 必須。将来の一般化に向けて意味のないフラグとして作成。
    * `--assume-no-padding`: 必須。将来の一般化に向けて意味のないフラグとして作成。
    * `--piece-rectangle-width`: 1以上の整数。各ピースの幅を表す。
    * `--piece-rectangle-height`: 1以上の整数。各ピースの高さを表す。
    * `--piece-rounding-policy cut`: 画像寸法がピース寸法の整数倍でない場合、右端・下端の余りを切り落とす。省略時はエラー。
    * `--input`: 画像ファイルのパス。PNG/JPEGをサポート。好きな数指定できる。
    * `--solved-information-output-format [text/json]`
        * `text`: シャッフルされた画像のピースについて、上からZ字になるように1, 2, 3, ... と番号を振っていき、解の構築に完了したら各ピースがどこに行ったか表示する。
        * `json`: `text` と同じだが、情報がフィールドごとに分割された機械可読なJSONで表示する。
    * `--output [*.png]` : `--input` の k 番目の結果をPNGでどこに出力するか。`--input` と正確に同じ数要求。

## 依存の制限

* 可能な限りRustで実装しなければならない。
* WebGPUを使用する場合、 `wgpu` クレートを使用すること。
* CLIのargument parsingには `clap` を使用し、 `derive` フィーチャーを有効にすること。

## ビルド・実行

Rust edition 2024 に対応するツールチェーンでビルドする。

```sh
cargo build --release -p kzigsaw-solver-cli
./target/release/kzigsaw-solver-cli help
```

現在は core / parallelization / cli の3クレートを実装している。
webapp は将来の実装予定で、今回の workspace には含まない。

### ピース寸法の候補を調べる

```sh
./target/release/kzigsaw-solver-cli detect-edge \
  --input shuffled.png --output-format json
```

`--input` は繰り返し指定できる。`--output-format` は `text`（既定）または `json`。
各軸で画像寸法の約数を調べ、周期的な境界の色差を周囲と比較する。
各軸上位5候補の組み合わせから上位10組を返す。スコアは相対的な境界の強さであり、正解確率ではない。
寸法1、単一行・列、単色画像など、比較に必要な境界や情報がない場合は候補を出せない。
候補なしの場合も正常終了し、理由を表示する。繰り返し模様では誤った寸法や倍数の候補が出ることがある。
`solve` への自動適用はしない。

### 並べ直す

```sh
./target/release/kzigsaw-solver-cli solve \
  --assume-all-pieces-are-same-shape \
  --assume-all-pieces-are-rectangle \
  --assume-no-padding \
  --piece-rectangle-width 32 \
  --piece-rectangle-height 32 \
  --input shuffled.png \
  --output restored.png \
  --solved-information-output-format json
```

各ピースは回転・反転されておらず、余白や欠落がなく、画像全体を同じ寸法の矩形で分割できることが前提。
寸法の指定は必須で、画像寸法が割り切れない場合はエラーとともに `--piece-rounding-policy cut` を案内する。
このオプションを指定すると左上を基準に右端・下端の余りを切り落とし、完全なピースだけを並べ直す。
例えば画像が103×82画素、ピースが20×20画素なら、出力は100×80画素（5列4行）となる。
切り落とした画素は出力に含まれず、ピース番号と移動情報も切り落とし後の格子を基準とする。
画像が1ピースより小さい場合は `cut` 指定時もエラーになる。
割り切れる場合は指定の有無によらず入力と同じ行数・列数を使う。
PNG/JPEG 入力を受け付け、出力は `.png` に限る。ピースの画素と透明度はそのまま移動する。

複数画像は `--input a.png --input b.jpg --output a-solved.png --output b-solved.png` のように指定する。
入力と出力を指定順に対応させ、各パズルは独立に解く。入力と出力の個数は一致させる。
出力先の親ディレクトリは事前に作成すること。既存出力は上書きするが、入力画像への上書きと出力先の重複は拒否する。
全入力の読み込みと寸法検証を終えてから出力を開始する。
後続画像の保存に失敗した場合、先に保存した画像は残る。

標準出力には解情報だけを出し、進捗・エラーは標準エラーに出す。
`--solved-information-output-format` は `text`（既定）または `json`。
番号は入力画像の左上から行優先（Z字）で1始まり、座標も1始まりで `(行, 列)`。
テキストの `piece 1: (1, 1) -> (2, 3)` は、入力左上のピースが出力の2行3列へ移動したことを表す。

JSON は単一入力でも配列で返す。各要素は次の形式（以下は1ピースの場合）：

```json
[
  {
    "input": "shuffled.png",
    "output": "restored.png",
    "columns": 1,
    "rows": 1,
    "piece_width": 32,
    "piece_height": 32,
    "pieces": [
      {
        "piece_id": 1,
        "source": { "row": 1, "column": 1 },
        "destination": { "row": 1, "column": 1 }
      }
    ]
  }
]
```

`detect-edge` の JSON も入力ごとの配列で、各要素に `input`、`candidates`、`reason` を含む。
候補は `piece_width`、`piece_height`、`columns`、`rows`、`score` を持ち、候補があると `reason` は `null`。
実行時エラーは終了コード1、引数の構文エラーは2で終了する。

## 探索方式と制限

RGB の境界画素の二乗色差と、境界をまたぐ勾配の不連続を評価する。
色差の重みは1、勾配差は0.5。寸法1の軸は色差だけで評価する。
全ピース対の隣接コストを Rayon で並列計算した後、幅64のビーム探索で配置する。
併せて、他の候補よりも明確に自然な隣接関係を優先してピース群を結合し、
重複せず指定の行列内に収まる配置が完成した場合は、ビーム探索の結果と比較する。
評価値が低い配置から、任意の2ピースを交換する局所改善を最大20巡行う。
これにより最初の64ピースに正しい左上ピースがない場合も復元できる。
同点では入力のピース番号順を優先する。
同じ入力・寸法では同じ結果となる。CPU のみで動作し、GPU は不要。

数百ピース程度を対象とする近似探索であり、正解や評価値の大域最適は保証しない。
単色、似た模様、繰り返し、シャープな輪郭、圧縮ノイズでは誤配置が起こり得る。
透明度は評価に使わないため、透明部分の RGB も評価に影響する。
隣接表のメモリはピース数 N に対して約 `16 * N * N` バイト、境界比較の計算量も二乗で増える。
ピース群の結合でも二乗個の隣接候補を保持するため、実際の使用メモリは隣接表より大きい。

## 検証

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p kzigsaw-solver-parallelization --example benchmark
```

テストは既知の配置の復元、透明度の保存、単一ピース・単一行・単一列、境界寸法候補、
PNG/JPEG 入出力、複数入力、JSON とテキストの移動情報、引数・画像・保存エラーを確認する。
ベンチマークは固定の合成画像2種類（160×160画素）を8×8画素の400ピースに分割し、
固定の順列でシャッフルして実行する。正しい絶対位置へ戻ったピースの割合と処理時間を表示する。
これは自然画像一般の復元精度を示す評価ではない。

開発環境での release ビルド実測（2026-10-18、画像生成・ファイルI/O・ビルド時間を除く）：

| 合成画像 | 処理時間 | 正しい絶対位置 | 正しい隣接関係 |
| --- | ---: | ---: | ---: |
| 線形グラデーション | 約65ms | 400/400 (100%) | 760/760 |
| 周期的な滑らかな模様 | 約178ms | 0/400 (0%) | 296/760 |

周期的な模様は現在の探索では大きく誤配置する例である。上記の数値は入力と実行環境に依存し、
写真一般の精度や性能を保証しない。各画像で結果を確認すること。
