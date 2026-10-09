# fin-primitives

[English](README.md) | 简体中文 | [日本語](README.ja.md) | [한국어](README.ko.md)

**一个 Rust 量化交易基础库，提供每个交易程序都需要的基础组件：拒绝非法值的精确价格、订单簿（order book）、由成交生成的 K 线、700+ 个技术指标、期权定价和风控限额。**

适合正在用 Rust 编写交易机器人、回测框架、行情数据工具或量化研究代码，又不想把同样的组件重写（再重新调试）一遍的开发者。

<p align="center">
  <a href="https://crates.io/crates/fin-primitives"><img alt="crates.io 版本" src="https://img.shields.io/crates/v/fin-primitives.svg"></a>
  <a href="https://docs.rs/fin-primitives"><img alt="docs.rs" src="https://docs.rs/fin-primitives/badge.svg"></a>
  <a href="https://gitlab.com/mattbusel/fin-primitives/-/blob/main/LICENSE"><img alt="MIT 许可证" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
</p>

<p align="center">
  <img alt="真实的终端录屏：cargo run --example order_book 打印出 BTC-USD 深度阶梯并拒绝了两条错误的更新，option_chain 打印出带希腊字母的看涨和看跌期权价格，position_risk 展示一个账本触发了 4% 的回撤限额" src="assets/demo.gif" width="100%">
</p>

## 安装

```bash
cargo add fin-primitives rust_decimal rust_decimal_macros
```

这是一个库，不需要在系统里安装任何东西。需要 Rust 1.81 或更高版本（在 CI 中验证）。
`rust_decimal_macros` 提供示例里用到的 `dec!(64250.50)` 字面量。

## 15 行上手

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

指标在积累到足够的 K 线之前（这里是前 14 根）会返回 `Unavailable`，绝不会编造一个 0 或 NaN。
这段代码会由 `cargo test --doc` 编译并运行。

## 为什么选这个 crate，而不是 `ta`、`yata` 或 TA-Lib

在同样的 100,000 个收盘价上实测（[bench/competitors](bench/competitors/README.md)、
[bench/talib_compare](bench/talib_compare/README.md)）：

- **精确的十进制数**：价格、数量、K 线和指标都是 `rust_decimal::Decimal`，所以
  `0.1 + 0.2` 就是 `0.3`，盈亏（P&L）精确到分。SMA 和 EMA 与 `ta`、`yata` 的结果
  相差在 1e-12 以内；在两边定义相同的 22 个指标上，与 TA-Lib 相差在 2e-11 以内或更小。
- **采用 TA-Lib 的定义**：`Rsi` 是 Wilder 的 RSI，与 TA-Lib 一致。`ta` 的 RSI 使用了
  不同的平滑系数，在同样的数据上最多相差 20 个 RSI 点。
- **输入有校验，错误有类型**：零价格、交叉的订单簿或跳号的序列号都会得到一个 `Err`，
  而不是悄无声息地给出一个错误数字。库代码中没有 `unwrap`。
- **一个 crate 覆盖整个流程**：订单簿、tick 转 K 线、700+ 个指标、持仓、
  回撤和 VaR 限额、带希腊字母的 Black-Scholes。
- **代价是速度**：每次指标更新，`ta` 和 `yata` 要快 40 到 160 倍
  （f64 对比 96 位十进制运算）。fin-primitives 每个核心每秒仍能完成 400 万到 2500 万次
  更新。如果你只需要处理很长的 float64 历史数据，用它们或 TA-Lib 就好。

## 已经在用 `ta`、`yata` 或 `wickra`？

继续用就行。打开对应的 feature，它们的指标就能直接读取 fin-primitives 的 K 线，
它们的 K 线类型也能转换成 fin-primitives 的类型：

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

`cargo run --example with_ta --features ta` 会运行完整版本。反方向也可以：
`BarInput::try_from(&ta::DataItem)`、`BarInput::try_from(&yata::core::Candle)` 和
`BarInput::try_from(&wickra_core::Candle)` 会把它们的 K 线转换成 fin-primitives 的输入。

## Feature 开关

| feature | 默认 | 新增内容 |
|---|---|---|
| `serde` | 是 | 数据类型上的 `Serialize`/`Deserialize`；反序列化时会重新执行校验 |
| `async` | 是 | `async_signals`：一个通过 channel 运行信号流水线的 Tokio 任务 |
| `ta` | 否 | 与 [`ta`](https://crates.io/crates/ta) crate 互操作 |
| `yata` | 否 | 与 [`yata`](https://crates.io/crates/yata) crate 互操作 |
| `wickra` | 否 | 与 [`wickra-core`](https://crates.io/crates/wickra-core) 互操作（Rust 1.86+） |
| `arrow` | 否 | K 线与 Apache Arrow `RecordBatch` 互相转换，价格为精确的 `Decimal128`（Rust 1.88+） |
| `python` | 否 | PyO3 绑定；`cd python && maturin develop --features python`（Rust 1.83+） |

`default-features = false` 时只保留 `rust_decimal`、`chrono`、`thiserror`、`libm` 和 `statrs`。
它也能编译到 `wasm32-unknown-unknown`。

## 工作原理

成交以 `Tick` 的形式进来。`OhlcvAggregator::push_tick` 把每笔成交放进对应的时间桶，
一旦下一个时间桶的成交到达，就立刻交还已完成的 K 线。K 线再喂给指标，
指标在历史数据不够时会返回 `Unavailable`，而不是编造一个数字。

<p align="center"><img alt="动画示意图：24 笔真实的 ETH-USD tick 落入一分钟的时间桶；当 tick 仍在未结束的桶里时 push_tick 返回空的 Vec，当第一笔 15:01 的 tick 到达时，返回已完成的 15:00 K 线（开 3186.17，高 3190.94，低 3182.16，收 3190.94，12 笔 tick）；RSI(14) 在第 1 到 14 根 K 线返回 Unavailable，在第 15 根返回 71.72" src="docs/img/ticks-to-bars.svg" width="100%"></p>

资金也是同样的思路：成交和价格标记进入 `PositionLedger`，它的账户价值进入 `RiskMonitor`，
每一条被触发的规则都会以 `RiskBreach` 的形式返回，供你采取行动。

<p align="center"><img alt="position_risk 示例的动画示意图：成交和标记进入 PositionLedger，net_liquidation_value 进入 RiskMonitor，其中有 4% 最大回撤规则和 96,600 的权益下限；权益在 101,237 达到峰值，在第 8 个事件（MSFT 标记为 371.50）跌到 96,507，回撤 4.67%，两条规则都被触发" src="docs/img/risk-check.svg" width="100%"></p>

## 示例

[`examples/`](examples/) 中附带六个程序。无需网络，无需 API 密钥，每次运行输出都一样。

| 运行 | 得到 |
|---|---|
| `cargo run --example order_book` | BTC-USD 深度阶梯、价差、微观价格（micro-price）、5 BTC 市价买入的成本，以及两条被拒绝的错误更新 |
| `cargo run --example candles` | 337 笔成交合成 32 根一分钟 K 线，并画出 EMA(9) 和 RSI(14) |
| `cargo run --example position_risk` | 一个按市值计价的交易时段，回撤规则和权益下限被触发 |
| `cargo run --example option_chain` | 带希腊字母的 Black-Scholes 期权链，以及隐含波动率的往返计算 |
| `cargo run --example value_at_risk` | 一个 $1M 账户的一日和十日 VaR：历史法、参数法和蒙特卡洛法，置信度 95%、97.5% 和 99% |
| `cargo run --example with_ta --features ta` | 由 fin-primitives 把 tick 合成 K 线，再喂给 `ta` 的 MACD 和布林带，以及 fin-primitives 的 RSI |

**`order_book`**：订单簿知道自己的价差和中间价，并拒绝会导致交叉或跳过序列号的更新。

<p align="center"><img alt="cargo run --example order_book 的终端输出：带深度条的 7 档 BTC-USD 阶梯、订单簿统计，以及两条被拒绝的增量更新" src="assets/term-order_book.png" width="760"></p>

**`position_risk`**（真实输出，运行于 2026-09-28，`NO_COLOR=1`）：

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

（为节省篇幅省略了第 1 到 4 行和第 9 到 10 行；上面的示意图播放了完整的交易时段。）

<details>
<summary><b>candles</b> 和 <b>option_chain</b> 的输出</summary>

<br>

<p align="center"><img alt="cargo run --example candles 的终端输出：32 根 K 线的蜡烛图，叠加 EMA 线，下面是包含预热行的 OHLCV、EMA 和 RSI 数值表" src="assets/term-candles.png" width="720"></p>

<p align="center"><img alt="cargo run --example option_chain 的终端输出：行权价 195 到 230 的看涨和看跌期权价格、delta、theta、gamma 和 vega，然后是隐含波动率还原出 28%" src="assets/term-option_chain.png" width="600"></p>

</details>

## 3 步上手

**1. 新建项目并添加 crate**

```bash
cargo new book-demo && cd book-demo
cargo add fin-primitives rust_decimal rust_decimal_macros
```

**2. 把下面的代码放进 `src/main.rs`**

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

**3. 运行**

```bash
cargo run
```

你会看到：

```text
Price::new(-5) -> Price must be positive, got -5
spread         0.50
mid price      64250.25
buy 1 BTC at   64250.58 average
```

负价格永远进不了你的程序，订单簿知道自己的价差和中间价，
"买 1 BTC 要花多少钱"只需要一次调用（0.842 BTC 以最优卖价成交，
其余 0.158 在更高一档成交）。这段程序在本仓库中同样会由
`cargo test --doc` 编译并运行。

## 与 TA-Lib 对比

把同样的 100,000 根带种子的 OHLCV K 线分别送进两个库（[方法、脚本和完整表格](bench/talib_compare/README.md)；
i7-13700KF，TA-Lib 0.8.1）。在定义一致的地方，数值相差在 2e-11 以内或更小；
fin-primitives 使用精确十进制运算，所以产生舍入误差的是 float64 那一方。
有两个定义是刻意不同的：`Atr` 是真实波幅的简单平均（等于 TA-Lib 的
`SMA(TRANGE)`，而不是 Wilder 的 `ATR`），`Obv` 从 0 开始，而不是从第一根
K 线的成交量开始。TA-Lib 快得多：它的批量函数每根 K 线耗时 0.2 到 12 ns，
大约只有 fin-primitives 精确十进制 `update()` 的 1/100。要处理很长的 float64 历史数据，
用 TA-Lib；需要精确数值、逐根 K 线的状态并且不依赖 C 库，用 fin-primitives。

| 指标 | 与 TA-Lib 的最大差值（第 1,000 根 K 线之后） | fin-primitives，每根 K 线 ns | TA-Lib 批量，每根 K 线 ns |
|---|---|---|---|
| SMA(20) / EMA(20) | 9.8e-13 / 5.7e-14 | 74 / 149 | 0.8 / 1.3 |
| RSI(14) | 1.8e-13 | 252 | 2.1 |
| MACD(12,26,9) 柱状图 | 5.0e-14（初始化方式不同：前几根 K 线相差 0.036） | 362 | 1.7 |
| 布林带 Bollinger(20, 2) | 1.6e-11 | 1023 | 2.7 |
| ATR(14) | 等于 TA-Lib `SMA(TRANGE, 14)`，相差 1.2e-14 | 86 | 0.8 |
| 快速随机指标 %K / %D (14, 3) | 6.7e-13 / 1.3e-12 | 174 / 259 | 7.7 |
| Williams %R(14) / CCI(20) / MFI(14) | 6.7e-13 / 1.0e-11 / 1.3e-12 | 170 / 1060 / 589 | 1.9 / 12.2 / 5.9 |
| ADX(14) | 3.7e-13，float64 会在涨跌几乎完全持平时分出差别，那些情况除外 | 794 | 6.2 |
| OBV | 与 TA-Lib `OBV - volume[0]` 完全相等 | 13 | 2.8 |

## 文档

| 阅读 | 内容 |
|---|---|
| [docs.rs/fin-primitives](https://docs.rs/fin-primitives) | 每个类型和方法，附带可编译的示例 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 各模块如何衔接、每个模块保证什么、设计规则、编写自己的指标或风控规则 |
| [docs/REFERENCE.md](docs/REFERENCE.md) | 模块指南（指标、序列分析、账本、回撤、归因、希腊字母、市场状态、回测器、蒙特卡洛、因子模型、收益率曲线等）、数学定义、API 列表 |
| [docs/TESTING.md](docs/TESTING.md) | 运行测试和基准测试、当前测试状态 |
| [CHANGELOG.md](CHANGELOG.md) | 每个版本的改动 |
| [项目网站](https://fin-primitives.vercel.app/) | 以网页形式呈现的同一份概览 |

可选的 Python 绑定在 `python` feature 之后：`cd python && maturin develop --release`，
然后运行 `python test_smoke.py`。

> 这是一个研究和工程用的库。它不会下单，这里的任何内容都不构成投资建议。

## 参与贡献

欢迎提交 issue 和 pull request。公开项需要 `///` 文档，可能失败的代码要返回
`Result`（测试之外不使用 `unwrap`、`expect` 或 `panic!`），新行为需要配套测试。
提交 PR 前请运行 `cargo fmt`、`cargo clippy` 和 `cargo test --doc`。详见
[CONTRIBUTING.md](CONTRIBUTING.md)。

## 许可证与相关项目

MIT，见 [LICENSE](LICENSE)。[fin-stream](https://gitlab.com/mattbusel/fin-stream) 基于
这个 crate 构建：它把 Binance、Coinbase、Alpaca 和 Polygon 的实时成交消息转换成 tick、
K 线和订单簿。
