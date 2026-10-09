# fin-primitives

[English](README.md) | [简体中文](README.zh-CN.md) | 日本語 | [한국어](README.ko.md)

**どんなトレーディングプログラムにも必要な基本部品をそろえた Rust のライブラリ：不正な値を受け付けない正確な価格、板（オーダーブック）、約定から作るローソク足、700 以上のテクニカル指標、オプション価格計算、リスク制限。**

トレーディングボット、バックテスター、マーケットデータツール、クオンツ研究のコードを Rust で書いていて、同じ部品を作り直し（そしてデバッグし直し）たくない開発者向けです。

<p align="center">
  <a href="https://crates.io/crates/fin-primitives"><img alt="crates.io のバージョン" src="https://img.shields.io/crates/v/fin-primitives.svg"></a>
  <a href="https://docs.rs/fin-primitives"><img alt="docs.rs" src="https://docs.rs/fin-primitives/badge.svg"></a>
  <a href="https://gitlab.com/mattbusel/fin-primitives/-/blob/main/LICENSE"><img alt="MIT ライセンス" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
</p>

<p align="center">
  <img alt="実際のターミナルセッション：cargo run --example order_book が BTC-USD の板を表示して 2 件の不正な更新を拒否し、option_chain がグリークス付きのコールとプットの価格を表示し、position_risk ではポジション台帳が 4% のドローダウン制限に引っかかる" src="assets/demo.gif" width="100%">
</p>

## インストール

```bash
cargo add fin-primitives rust_decimal rust_decimal_macros
```

ライブラリなので、システムにインストールするものはありません。Rust 1.81 以降が必要です（CI で確認済み）。
`rust_decimal_macros` は、サンプルで使っている `dec!(64250.50)` リテラルのためのものです。

## 15 行で試す

```rust
use fin_primitives::signals::{indicators::Rsi, BarInput, Signal, SignalValue};
use fin_primitives::types::Price;
use rust_decimal_macros::dec;

fn main() -> Result<(), fin_primitives::FinError> {
    assert!(Price::new(dec!(-1)).is_err()); // bad values never get in

    let mut rsi = Rsi::new("rsi14", 14)?;
    let closes = [dec!(44.34), dec!(44.09), dec!(44.15), dec!(43.61), dec!(44.33), dec!(44.83), dec!(45.10), dec!(45.42),
                  dec!(45.84), dec!(46.08), dec!(45.89), dec!(46.03), dec!(45.61), dec!(46.28), dec!(46.28), dec!(46.00)];
    for close in closes {
        if let SignalValue::Scalar(v) = rsi.update(&BarInput::from_close(close))? {
            println!("RSI(14) = {}", v.round_dp(2)); // 70.46, then 66.25
        }
    }
    Ok(())
}
```

指標は十分な本数の足がそろうまで（ここでは最初の 14 本）`Unavailable` を返し、
0 や NaN をでっち上げることはありません。このコードブロックは `cargo test --doc` でコンパイル・実行されます。

## `ta`、`yata`、TA-Lib ではなくこのクレートを選ぶ理由

同じ 100,000 本の終値で計測しました（[bench/competitors](bench/competitors/README.md)、
[bench/talib_compare](bench/talib_compare/README.md)）。

- **正確な 10 進数**：価格、数量、足、指標はすべて `rust_decimal::Decimal` なので、
  `0.1 + 0.2` は `0.3` になり、損益（P&L）は 1 セント単位まで合います。SMA と EMA は `ta`、`yata` と
  1e-12 以内で一致し、定義が同じ 22 個の指標では TA-Lib と 2e-11 以内で一致します。
- **TA-Lib の定義に準拠**：`Rsi` は TA-Lib と同じく Wilder の RSI です。`ta` の RSI は
  別の係数で平滑化しており、同じデータで最大 20 ポイントの差が出ました。
- **入力はチェック済み、エラーは型付き**：ゼロの価格、クロスした板、飛んだシーケンス番号は
  `Err` になり、黙って間違った数字を返すことはありません。ライブラリのコードに `unwrap` はありません。
- **一連の処理がひとつのクレートで完結**：板、ティックから足への変換、700 以上の指標、ポジション、
  ドローダウンと VaR の制限、グリークス付きのブラック・ショールズ。
- **その代わり速度は犠牲に**：指標の更新 1 回あたり、`ta` と `yata` は 40 倍から 160 倍速い
  （f64 と 96 ビット 10 進演算の差）です。それでも fin-primitives は 1 コアあたり毎秒 400 万から 2,500 万回の
  更新をこなします。長い float64 の履歴を処理するだけなら、それらか TA-Lib を使ってください。

## すでに `ta`、`yata`、`wickra` を使っている場合

そのまま使い続けてください。対応する feature を有効にすれば、それらの指標が fin-primitives の足を
直接読めるようになり、それらの足の型も fin-primitives の型に変換できます。

```toml
fin-primitives = { version = "2.15", features = ["ta"] }   # or "yata", "wickra", "arrow"
```

```rust,ignore
use ta::indicators::MovingAverageConvergenceDivergence;
use ta::Next;

let mut macd = MovingAverageConvergenceDivergence::new(12, 26, 9)?;
for bar in aggregator.push_tick(&tick)? {          // fin_primitives::ohlcv::OhlcvBar
    let m = macd.next(&bar);                        // ta reads the bar through ta::Close
    let r = rsi.update_bar(&bar)?;                  // fin-primitives Wilder RSI, same bar
}
```

`cargo run --example with_ta --features ta` で完全版を実行できます。逆方向の変換では、
`BarInput::try_from(&ta::DataItem)`、`BarInput::try_from(&yata::core::Candle)`、
`BarInput::try_from(&wickra_core::Candle)` がそれぞれの足を fin-primitives の入力に変換します。

## Feature フラグ

| feature | デフォルト | 追加されるもの |
|---|---|---|
| `serde` | あり | データ型への `Serialize`/`Deserialize`。デシリアライズ時にはバリデーションが再実行される |
| `async` | あり | `async_signals`：チャネル経由でシグナルパイプラインを動かす Tokio タスク |
| `ta` | なし | [`ta`](https://crates.io/crates/ta) クレートとの相互運用 |
| `yata` | なし | [`yata`](https://crates.io/crates/yata) クレートとの相互運用 |
| `wickra` | なし | [`wickra-core`](https://crates.io/crates/wickra-core) との相互運用（Rust 1.86+） |
| `arrow` | なし | 足と Apache Arrow `RecordBatch` の相互変換、価格は正確な `Decimal128`（Rust 1.88+） |
| `python` | なし | PyO3 バインディング。`cd python && maturin develop --features python`（Rust 1.83+） |

`default-features = false` にすると、残るのは `rust_decimal`、`chrono`、`thiserror`、`libm`、`statrs` だけです。
`wasm32-unknown-unknown` 向けにもビルドできます。

## 仕組み

約定は `Tick` として入ってきます。`OhlcvAggregator::push_tick` は各約定を時間ごとのバケットに入れ、
次のバケットの約定が届いた瞬間に、完成したローソク足を返します。ローソク足は指標に渡され、
指標は履歴が足りないうちは数字をでっち上げずに `Unavailable` を返します。

<p align="center"><img alt="アニメーション図：実際の ETH-USD のティック 24 件が 1 分ごとのバケットに入る。ティックが開いているバケットにとどまる間、push_tick は空の Vec を返し、15:01 の最初のティックが届いたときに完成した 15:00 の足（始値 3186.17、高値 3190.94、安値 3182.16、終値 3190.94、12 ティック）を返す。RSI(14) は 1 本目から 14 本目までは Unavailable を返し、15 本目で 71.72 を返す" src="docs/img/ticks-to-bars.svg" width="100%"></p>

お金の流れも同じです。約定と時価評価は `PositionLedger` に入り、その口座評価額が `RiskMonitor` に入り、
破られたルールはすべて、対応に使える `RiskBreach` として返ってきます。

<p align="center"><img alt="position_risk サンプルのアニメーション図：約定と時価評価が PositionLedger に入り、net_liquidation_value が最大ドローダウン 4% のルールと 96,600 の資産下限を持つ RiskMonitor に入る。資産は 101,237 でピークに達し、イベント 8（MSFT を 371.50 で評価）で 96,507 まで下がってドローダウンは 4.67% となり、両方のルールが発動する" src="docs/img/risk-check.svg" width="100%"></p>

## サンプル

[`examples/`](examples/) に 6 つのプログラムが入っています。ネットワークも API キーも不要で、毎回同じ出力になります。

| 実行するコマンド | 得られるもの |
|---|---|
| `cargo run --example order_book` | BTC-USD の板、スプレッド、マイクロプライス、5 BTC を成行買いしたときのコスト、そして拒否される 2 件の不正な更新 |
| `cargo run --example candles` | 337 件の約定をまとめた 32 本の 1 分足、EMA(9) と RSI(14) 付きで描画 |
| `cargo run --example position_risk` | 時価評価されるトレーディングセッション、ドローダウンのルールと資産下限が発動 |
| `cargo run --example option_chain` | グリークス付きのブラック・ショールズのオプションチェーンと、インプライド・ボラティリティの往復計算 |
| `cargo run --example value_at_risk` | $1M のブックの 1 日と 10 日の VaR：ヒストリカル法、パラメトリック法、モンテカルロ法、信頼水準 95%、97.5%、99% |
| `cargo run --example with_ta --features ta` | fin-primitives でティックを足にまとめ、`ta` の MACD とボリンジャーバンド、そして fin-primitives の RSI に渡す |

**`order_book`**：板は自分のスプレッドと仲値を把握しており、クロスを起こす更新やシーケンス番号が飛んだ更新を拒否します。

<p align="center"><img alt="cargo run --example order_book のターミナル出力：深さバー付きの 7 段の BTC-USD 板、板の統計、拒否された 2 件の差分" src="assets/term-order_book.png" width="760"></p>

**`position_risk`**（実際の出力、2026-09-28 に実行、`NO_COLOR=1`）：

```text
  #   event                        equity  drawdown  risk
  5   sell 100 AAPL @ 179.10   101,237.00     0.00%  ok
  6   mark MSFT 398.10          99,865.00     1.36%  ok
  7   mark AAPL 166.80          98,635.00     2.57%  ok
  8   mark MSFT 371.50          96,507.00     4.67%  BREACH max_drawdown, min_equity
        max_drawdown: drawdown 4.67% > 4.00%
        min_equity: equity 96507.00 < floor 96600

  rejected  buy 1000 AAPL  Insufficient funds: need 171351.00, have 79866.00
```

（長くなるので 1 から 4 行目と 9 から 10 行目は省略しています。上の図でセッション全体を再生しています。）

<details>
<summary><b>candles</b> と <b>option_chain</b> の出力</summary>

<br>

<p align="center"><img alt="cargo run --example candles のターミナル出力：EMA を重ねた 32 本のローソク足チャートと、その下にウォームアップ行を含む OHLCV、EMA、RSI の値の表" src="assets/term-candles.png" width="720"></p>

<p align="center"><img alt="cargo run --example option_chain のターミナル出力：行使価格 195 から 230 までのコールとプットの価格、デルタ、セータ、ガンマ、ベガ、そしてインプライド・ボラティリティ 28% の復元" src="assets/term-option_chain.png" width="600"></p>

</details>

## 3 ステップで使う

**1. プロジェクトを作ってクレートを追加する**

```bash
cargo new book-demo && cd book-demo
cargo add fin-primitives rust_decimal rust_decimal_macros
```

**2. これを `src/main.rs` に書く**

```rust
use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::types::{Price, Quantity, Side, Symbol};
use rust_decimal_macros::dec;

fn main() -> Result<(), fin_primitives::FinError> {
    // 1. Values are checked when you create them.
    println!("Price::new(-5) -> {}", Price::new(dec!(-5)).unwrap_err());

    // 2. Build a small BTC-USD order book from four quotes.
    let mut book = OrderBook::new(Symbol::new("BTC-USD")?);
    let quotes = [
        (Side::Ask, dec!(64250.50), dec!(0.842)),
        (Side::Ask, dec!(64251.00), dec!(1.310)),
        (Side::Bid, dec!(64250.00), dec!(1.204)),
        (Side::Bid, dec!(64249.50), dec!(0.655)),
    ];
    for (seq, (side, price, qty)) in (1..).zip(quotes) {
        book.apply_delta(BookDelta {
            side,
            price: Price::new(price)?,
            quantity: Quantity::new(qty)?,
            action: DeltaAction::Set,
            sequence: seq,
        })?;
    }

    // 3. Ask it questions.
    println!("spread         {}", book.spread().unwrap_or_default());
    println!("mid price      {}", book.mid_price().unwrap_or_default());
    let one_btc = Quantity::new(dec!(1))?;
    println!("buy 1 BTC at   {} average", book.vwap_for_qty(Side::Ask, one_btc)?.round_dp(2));
    Ok(())
}
```

**3. 実行する**

```bash
cargo run
```

次のように表示されます。

```text
Price::new(-5) -> Price must be positive, got -5
spread         0.50
mid price      64250.25
buy 1 BTC at   64250.58 average
```

マイナスの価格がプログラムに入り込むことはなく、板は自分のスプレッドと仲値を把握していて、
「1 BTC 買うといくらかかるか」は 1 回の呼び出しでわかります（0.842 BTC が最良売り気配で約定し、
残りの 0.158 はひとつ上の価格帯で約定）。このプログラムそのものも、このリポジトリの
`cargo test --doc` でコンパイル・実行されています。

## TA-Lib との比較

同じ 100,000 本のシード付き OHLCV 足を両方のライブラリに通しました（[手法、スクリプト、全表](bench/talib_compare/README.md)、
i7-13700KF、TA-Lib 0.8.1）。定義が同じ指標では、値は 2e-11 以内で一致します。
fin-primitives は正確な 10 進数で計算するので、丸め誤差が出るのは float64 の側です。
2 つの定義は意図的に変えています。`Atr` は真の値幅（true range）の単純平均で（TA-Lib の
`SMA(TRANGE)` と同じで、Wilder の `ATR` ではありません）、`Obv` は最初の足の出来高ではなく
0 から始まります。TA-Lib のほうがずっと速く、バッチ関数は 1 本あたり 0.2 から 12 ns で、
fin-primitives の正確な 10 進演算による `update()` のおよそ 100 分の 1 です。長い float64 の履歴を
処理するなら TA-Lib を、正確な値、足ごとの状態管理、C 依存なしが欲しいなら fin-primitives を使ってください。

| 指標 | TA-Lib との最大差（1,000 本目以降） | fin-primitives、1 本あたり ns | TA-Lib バッチ、1 本あたり ns |
|---|---|---|---|
| SMA(20) / EMA(20) | 9.8e-13 / 5.7e-14 | 74 / 149 | 0.8 / 1.3 |
| RSI(14) | 1.8e-13 | 252 | 2.1 |
| MACD(12,26,9) ヒストグラム | 5.0e-14（初期化の方法が違うため、最初の数本では 0.036） | 362 | 1.7 |
| ボリンジャーバンド Bollinger(20, 2) | 1.6e-11 | 1023 | 2.7 |
| ATR(14) | TA-Lib の `SMA(TRANGE, 14)` と 1.2e-14 以内で一致 | 86 | 0.8 |
| ストキャスティクス・ファスト %K / %D (14, 3) | 6.7e-13 / 1.3e-12 | 174 / 259 | 7.7 |
| Williams %R(14) / CCI(20) / MFI(14) | 6.7e-13 / 1.0e-11 / 1.3e-12 | 170 / 1060 / 589 | 1.9 / 12.2 / 5.9 |
| ADX(14) | 3.7e-13。ただし上昇と下降がほぼ同値で float64 だけが差をつける箇所は除く | 794 | 6.2 |
| OBV | TA-Lib の `OBV - volume[0]` と完全に一致 | 13 | 2.8 |

## ドキュメント

| 読むもの | 内容 |
|---|---|
| [docs.rs/fin-primitives](https://docs.rs/fin-primitives) | すべての型とメソッド、コンパイルできるサンプル付き |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | モジュール同士のつながり、各モジュールが保証すること、設計ルール、独自の指標やリスクルールの書き方 |
| [docs/REFERENCE.md](docs/REFERENCE.md) | モジュールガイド（指標、時系列分析、台帳、ドローダウン、パフォーマンス要因分析、グリークス、相場レジーム、バックテスター、モンテカルロ、ファクターモデル、イールドカーブなど）、数式の定義、API 一覧 |
| [docs/TESTING.md](docs/TESTING.md) | テストとベンチマークの実行方法、現在のテスト状況 |
| [CHANGELOG.md](CHANGELOG.md) | 各バージョンの変更点 |
| [プロジェクトサイト](https://fin-primitives.vercel.app/) | 同じ概要をウェブページで |

オプションの Python バインディングは `python` feature で有効になります：`cd python && maturin develop --release`
を実行してから `python test_smoke.py` を実行してください。

> 研究・エンジニアリング用のライブラリです。注文は出さず、ここにある内容はいずれも投資助言ではありません。

## コントリビュート

Issue とプルリクエストを歓迎します。公開アイテムには `///` ドキュメントが必要で、失敗しうるコードは
`Result` を返し（テスト以外で `unwrap`、`expect`、`panic!` は使わない）、新しい振る舞いにはテストが必要です。
PR を出す前に `cargo fmt`、`cargo clippy`、`cargo test --doc` を実行してください。詳しくは
[CONTRIBUTING.md](CONTRIBUTING.md) を参照してください。

## ライセンスと関連プロジェクト

MIT。[LICENSE](LICENSE) を参照してください。[fin-stream](https://gitlab.com/mattbusel/fin-stream) は
このクレートの上に作られています。Binance、Coinbase、Alpaca、Polygon のリアルタイム約定メッセージを、ティック、
足、板に変換します。
