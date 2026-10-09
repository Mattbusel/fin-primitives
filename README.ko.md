# fin-primitives

[English](README.md) | [简体中文](README.zh-CN.md) | [日本語](README.ja.md) | 한국어

**모든 트레이딩 프로그램에 필요한 기본 부품을 모은 Rust 라이브러리: 잘못된 값을 거부하는 정확한 가격, 호가창(오더북), 체결로 만드는 캔들, 700개 이상의 기술적 지표, 옵션 가격 계산, 리스크 한도.**

트레이딩 봇, 백테스터, 시세 데이터 도구, 퀀트 리서치 코드를 Rust로 작성하면서 같은 부품을 다시 만들고(그리고 다시 디버깅하고) 싶지 않은 개발자를 위한 라이브러리입니다.

<p align="center">
  <a href="https://crates.io/crates/fin-primitives"><img alt="crates.io 버전" src="https://img.shields.io/crates/v/fin-primitives.svg"></a>
  <a href="https://docs.rs/fin-primitives"><img alt="docs.rs" src="https://docs.rs/fin-primitives/badge.svg"></a>
  <a href="https://gitlab.com/mattbusel/fin-primitives/-/blob/main/LICENSE"><img alt="MIT 라이선스" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
</p>

<p align="center">
  <img alt="실제 터미널 세션: cargo run --example order_book이 BTC-USD 호가 사다리를 출력하고 잘못된 업데이트 두 건을 거부하며, option_chain이 그릭스와 함께 콜·풋 가격을 출력하고, position_risk에서는 포지션 원장이 4% 드로다운 한도에 걸린다" src="assets/demo.gif" width="100%">
</p>

## 설치

```bash
cargo add fin-primitives rust_decimal rust_decimal_macros
```

라이브러리이므로 시스템에 따로 설치할 것은 없습니다. Rust 1.81 이상이 필요합니다(CI에서 확인).
`rust_decimal_macros`는 예제에서 쓰는 `dec!(64250.50)` 리터럴을 제공합니다.

## 15줄로 맛보기

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

지표는 봉이 충분히 쌓일 때까지(여기서는 처음 14개) `Unavailable`을 반환하며, 0이나 NaN을
지어내지 않습니다. 이 코드 블록은 `cargo test --doc`으로 컴파일되고 실행됩니다.

## `ta`, `yata`, TA-Lib 대신 이 크레이트를 쓰는 이유

같은 100,000개의 종가로 측정했습니다([bench/competitors](bench/competitors/README.md),
[bench/talib_compare](bench/talib_compare/README.md)):

- **정확한 10진수**: 가격, 수량, 봉, 지표가 모두 `rust_decimal::Decimal`이라서
  `0.1 + 0.2`는 `0.3`이고 손익(P&L)이 센트 단위까지 맞습니다. SMA와 EMA는 `ta`, `yata`와
  1e-12 이내로 일치하고, 정의가 같은 22개 지표에서는 TA-Lib과 2e-11 이내로 일치합니다.
- **TA-Lib 정의를 따름**: `Rsi`는 TA-Lib과 같은 Wilder의 RSI입니다. `ta`의 RSI는
  다른 계수로 평활화하며, 같은 데이터에서 RSI가 최대 20포인트 차이 났습니다.
- **입력은 검사하고, 오류는 타입으로**: 0인 가격, 교차된 호가창, 건너뛴
  시퀀스 번호는 조용히 틀린 숫자가 되지 않고 `Err`가 됩니다. 라이브러리 코드에는 `unwrap`이 없습니다.
- **전체 흐름을 크레이트 하나로**: 호가창, 틱에서 봉으로 변환, 700개 이상의 지표, 포지션,
  드로다운과 VaR 한도, 그릭스를 포함한 블랙-숄즈.
- **대가는 속도**: 지표 업데이트 한 번당 `ta`와 `yata`가 40배에서 160배 빠릅니다
  (f64 대 96비트 10진 연산). 그래도 fin-primitives는 코어당 초당 400만에서 2,500만 번의
  업데이트를 처리합니다. 긴 float64 이력만 처리한다면 그것들이나 TA-Lib을 쓰세요.

## 이미 `ta`, `yata`, `wickra`를 쓰고 있다면?

계속 쓰세요. 해당 feature를 켜면 그 라이브러리들의 지표가 fin-primitives 봉을
직접 읽고, 그쪽 봉 타입도 fin-primitives 타입으로 변환됩니다:

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

`cargo run --example with_ta --features ta`로 전체 버전을 실행할 수 있습니다. 반대 방향으로는
`BarInput::try_from(&ta::DataItem)`, `BarInput::try_from(&yata::core::Candle)`,
`BarInput::try_from(&wickra_core::Candle)`이 그쪽 봉을 fin-primitives 입력으로 바꿔 줍니다.

## Feature 플래그

| feature | 기본값 | 추가되는 것 |
|---|---|---|
| `serde` | 예 | 데이터 타입에 `Serialize`/`Deserialize`; 역직렬화할 때 검증을 다시 실행 |
| `async` | 예 | `async_signals`: 채널을 통해 시그널 파이프라인을 돌리는 Tokio 태스크 |
| `ta` | 아니요 | [`ta`](https://crates.io/crates/ta) 크레이트와 상호 운용 |
| `yata` | 아니요 | [`yata`](https://crates.io/crates/yata) 크레이트와 상호 운용 |
| `wickra` | 아니요 | [`wickra-core`](https://crates.io/crates/wickra-core)와 상호 운용(Rust 1.86+) |
| `arrow` | 아니요 | 봉과 Apache Arrow `RecordBatch` 간 변환, 가격은 정확한 `Decimal128`(Rust 1.88+) |
| `python` | 아니요 | PyO3 바인딩; `cd python && maturin develop --features python`(Rust 1.83+) |

`default-features = false`로 두면 `rust_decimal`, `chrono`, `thiserror`, `libm`, `statrs`만 남습니다.
`wasm32-unknown-unknown`용으로도 빌드됩니다.

## 동작 원리

체결은 `Tick`으로 들어옵니다. `OhlcvAggregator::push_tick`은 각 체결을 시간 버킷에 넣고,
다음 버킷의 체결이 도착하는 순간 완성된 캔들을 돌려줍니다. 캔들은 지표로 들어가고,
지표는 이력이 충분하지 않으면 숫자를 지어내는 대신 `Unavailable`을 반환합니다.

<p align="center"><img alt="애니메이션 다이어그램: 실제 ETH-USD 틱 24개가 1분 버킷에 들어간다. 틱이 열린 버킷에 머무는 동안 push_tick은 빈 Vec을 반환하고, 15:01의 첫 틱이 도착하면 완성된 15:00 봉(시가 3186.17, 고가 3190.94, 저가 3182.16, 종가 3190.94, 틱 12개)을 반환한다. RSI(14)는 1번부터 14번 봉까지 Unavailable을, 15번 봉에서 71.72를 반환한다" src="docs/img/ticks-to-bars.svg" width="100%"></p>

돈도 같은 방식으로 흐릅니다: 체결과 시가 평가가 `PositionLedger`로 들어가고, 그 계좌 가치가
`RiskMonitor`로 들어가며, 위반된 규칙은 모두 대응할 수 있는 `RiskBreach`로 돌아옵니다.

<p align="center"><img alt="position_risk 예제의 애니메이션 다이어그램: 체결과 시가 평가가 PositionLedger로 들어가고, net_liquidation_value가 최대 드로다운 4% 규칙과 96,600 자산 하한이 있는 RiskMonitor로 들어간다. 자산은 101,237에서 정점을 찍고 이벤트 8(MSFT를 371.50으로 평가)에서 96,507로 떨어져 드로다운 4.67%가 되며, 두 규칙이 모두 발동한다" src="docs/img/risk-check.svg" width="100%"></p>

## 예제

[`examples/`](examples/)에 프로그램 6개가 들어 있습니다. 네트워크도 API 키도 필요 없고, 실행할 때마다 출력이 같습니다.

| 실행 명령 | 결과 |
|---|---|
| `cargo run --example order_book` | BTC-USD 호가 사다리, 스프레드, 마이크로 프라이스, 5 BTC 시장가 매수 비용, 그리고 거부되는 잘못된 업데이트 두 건 |
| `cargo run --example candles` | 체결 337건을 1분 캔들 32개로 묶고 EMA(9)와 RSI(14)를 함께 그림 |
| `cargo run --example position_risk` | 시가 평가되는 트레이딩 세션, 드로다운 규칙과 자산 하한이 발동 |
| `cargo run --example option_chain` | 그릭스를 포함한 블랙-숄즈 옵션 체인과 내재 변동성 왕복 계산 |
| `cargo run --example value_at_risk` | $1M 북의 1일 및 10일 VaR: 역사적 시뮬레이션, 모수적 방법, 몬테카를로, 신뢰수준 95%, 97.5%, 99% |
| `cargo run --example with_ta --features ta` | fin-primitives가 틱을 봉으로 묶어 `ta`의 MACD와 볼린저 밴드, 그리고 fin-primitives의 RSI에 넘김 |

**`order_book`**: 호가창은 자신의 스프레드와 중간 가격을 알고 있고, 호가를 교차시키거나 시퀀스 번호를 건너뛰는 업데이트를 거부합니다.

<p align="center"><img alt="cargo run --example order_book의 터미널 출력: 깊이 막대가 있는 7단계 BTC-USD 호가 사다리, 호가창 통계, 거부된 델타 두 건" src="assets/term-order_book.png" width="760"></p>

**`position_risk`**(실제 출력, 2026-09-28 실행, `NO_COLOR=1`):

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

(길이 때문에 1~4행과 9~10행은 생략했습니다. 위 다이어그램에서 세션 전체를 재생합니다.)

<details>
<summary><b>candles</b>와 <b>option_chain</b> 출력</summary>

<br>

<p align="center"><img alt="cargo run --example candles의 터미널 출력: EMA를 겹친 봉 32개의 캔들 차트, 그 아래 워밍업 행을 포함한 OHLCV, EMA, RSI 값 표" src="assets/term-candles.png" width="720"></p>

<p align="center"><img alt="cargo run --example option_chain의 터미널 출력: 행사가 195부터 230까지의 콜·풋 가격, 델타, 세타, 감마, 베가, 그리고 내재 변동성 28% 복원" src="assets/term-option_chain.png" width="600"></p>

</details>

## 3단계로 사용하기

**1. 프로젝트를 만들고 크레이트 추가하기**

```bash
cargo new book-demo && cd book-demo
cargo add fin-primitives rust_decimal rust_decimal_macros
```

**2. 이 코드를 `src/main.rs`에 넣기**

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

**3. 실행하기**

```bash
cargo run
```

이렇게 출력됩니다:

```text
Price::new(-5) -> Price must be positive, got -5
spread         0.50
mid price      64250.25
buy 1 BTC at   64250.58 average
```

음수 가격은 프로그램에 절대 들어오지 못하고, 호가창은 자신의 스프레드와 중간 가격을 알며,
"1 BTC를 사면 얼마가 드는지"는 호출 한 번으로 알 수 있습니다(0.842 BTC는 최우선 매도 호가에서 체결되고,
나머지 0.158은 한 단계 위 호가에서 체결). 이 프로그램 그대로가 이 저장소의
`cargo test --doc`에서도 컴파일되고 실행됩니다.

## TA-Lib과 비교

같은 100,000개의 시드 고정 OHLCV 봉을 두 라이브러리에 똑같이 넣었습니다([방법, 스크립트, 전체 표](bench/talib_compare/README.md);
i7-13700KF, TA-Lib 0.8.1). 정의가 같은 경우 값은 2e-11 이내로 일치합니다.
fin-primitives는 정확한 10진수로 계산하므로, 반올림 오차가 생기는 쪽은 float64입니다.
두 가지 정의는 일부러 다르게 했습니다: `Atr`은 실제 범위(true range)의 단순 평균이고(TA-Lib의
`SMA(TRANGE)`와 같으며 Wilder의 `ATR`이 아님), `Obv`는 첫 봉의 거래량이 아니라
0에서 시작합니다. TA-Lib이 훨씬 빠릅니다: 배치 함수가 봉 하나당 0.2~12 ns로,
fin-primitives의 정확한 10진수 `update()`보다 약 100배 빠릅니다. 긴 float64 이력을 처리할 때는
TA-Lib을, 정확한 값과 봉 단위 상태 관리, C 의존성 없음이 필요할 때는 fin-primitives를 쓰세요.

| 지표 | TA-Lib 대비 최대 차이(1,000번째 봉 이후) | fin-primitives, 봉당 ns | TA-Lib 배치, 봉당 ns |
|---|---|---|---|
| SMA(20) / EMA(20) | 9.8e-13 / 5.7e-14 | 74 / 149 | 0.8 / 1.3 |
| RSI(14) | 1.8e-13 | 252 | 2.1 |
| MACD(12,26,9) 히스토그램 | 5.0e-14(초기화 방식이 달라 처음 몇 봉에서는 0.036) | 362 | 1.7 |
| 볼린저 밴드 Bollinger(20, 2) | 1.6e-11 | 1023 | 2.7 |
| ATR(14) | TA-Lib `SMA(TRANGE, 14)`와 1.2e-14 이내로 일치 | 86 | 0.8 |
| 패스트 스토캐스틱 %K / %D (14, 3) | 6.7e-13 / 1.3e-12 | 174 / 259 | 7.7 |
| Williams %R(14) / CCI(20) / MFI(14) | 6.7e-13 / 1.0e-11 / 1.3e-12 | 170 / 1060 / 589 | 1.9 / 12.2 / 5.9 |
| ADX(14) | 3.7e-13, 단 상승/하락이 거의 정확히 같아 float64만 차이를 내는 지점은 제외 | 794 | 6.2 |
| OBV | TA-Lib `OBV - volume[0]`과 정확히 일치 | 13 | 2.8 |

## 문서

| 문서 | 내용 |
|---|---|
| [docs.rs/fin-primitives](https://docs.rs/fin-primitives) | 모든 타입과 메서드, 컴파일되는 예제 포함 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 모듈 간 연결 방식, 각 모듈이 보장하는 것, 설계 규칙, 직접 지표나 리스크 규칙 작성하기 |
| [docs/REFERENCE.md](docs/REFERENCE.md) | 모듈 가이드(지표, 시계열 분석, 원장, 드로다운, 성과 기여도 분석, 그릭스, 시장 국면, 백테스터, 몬테카를로, 팩터 모델, 수익률 곡선 등), 수학적 정의, API 목록 |
| [docs/TESTING.md](docs/TESTING.md) | 테스트와 벤치마크 실행, 현재 테스트 상태 |
| [CHANGELOG.md](CHANGELOG.md) | 버전별 변경 사항 |
| [프로젝트 사이트](https://fin-primitives.vercel.app/) | 같은 개요를 웹 페이지로 |

선택 사항인 Python 바인딩은 `python` feature로 켭니다: `cd python && maturin develop --release`
다음에 `python test_smoke.py`를 실행하세요.

> 연구 및 엔지니어링용 라이브러리입니다. 주문을 내지 않으며, 여기 있는 어떤 내용도 투자 조언이 아닙니다.

## 기여하기

이슈와 풀 리퀘스트를 환영합니다. 공개 항목에는 `///` 문서가 필요하고, 실패할 수 있는 코드는
`Result`를 반환해야 하며(테스트 밖에서는 `unwrap`, `expect`, `panic!` 금지), 새 동작에는 테스트가 필요합니다.
PR을 열기 전에 `cargo fmt`, `cargo clippy`, `cargo test --doc`을 실행하세요.
[CONTRIBUTING.md](CONTRIBUTING.md)를 참고하세요.

## 라이선스와 관련 프로젝트

MIT, [LICENSE](LICENSE)를 참고하세요. [fin-stream](https://gitlab.com/mattbusel/fin-stream)은
이 크레이트 위에 만들어졌습니다: Binance, Coinbase, Alpaca, Polygon의 실시간 체결 메시지를 틱,
봉, 호가창으로 바꿔 줍니다.
