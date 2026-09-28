"""fin-primitives: ticks -> OhlcvAggregator::push_tick -> bars -> Signal warm-up. Animated.

Tick data: the first 24 ticks of `cargo run --example candles` (seeded, dumped 2026-09-28
through OhlcvAggregator with Timeframe::Minutes(1)). Bar values and the RSI numbers are
from the same example's printed output.
"""
import sys
from common import *

TICKS = [
    (0, 3186.17, 0.251), (3, 3187.69, 0.618), (5, 3185.85, 0.017), (12, 3185.58, 0.039),
    (18, 3185.43, 0.073), (23, 3182.88, 0.500), (26, 3184.03, 2.806), (35, 3184.85, 2.989),
    (38, 3182.16, 0.590), (41, 3185.02, 1.004), (49, 3188.17, 1.456), (58, 3190.94, 0.238),
    (61, 3192.16, 0.766), (63, 3192.48, 0.174), (72, 3194.95, 1.298), (81, 3197.53, 0.010),
    (90, 3200.22, 2.840), (95, 3200.79, 0.279), (99, 3199.94, 0.056), (102, 3200.36, 0.017),
    (104, 3201.16, 0.033), (112, 3201.85, 4.010), (118, 3203.13, 0.677), (127, 3203.11, 0.908),
]
DUR = 16.0
W, H = 960, 700

s = Svg(W, H, "Animated diagram of fin-primitives turning trades into candles. Twenty-four real ETH-USD ticks "
        "from the candles example land on a price and time chart split into one-minute buckets. Each tick goes "
        "through OhlcvAggregator::push_tick, which returns an empty Vec while the tick stays in the current "
        "bucket and returns the finished bar when a tick opens the next bucket: the 15:00 bar closes at "
        "open 3186.17, high 3190.94, low 3182.16, close 3190.94 from 12 ticks. Finished bars feed indicators "
        "through Signal::update; RSI(14) returns Unavailable for bars 1 to 14 and its first value, 71.72, on bar 15.")

s.text(32, 44, "How trades become candles and indicators", 24, font=SERIF)
s.text(32, 70, "24 real ticks from cargo run --example candles, one-minute bars, replayed", 14, MUTED)

# ---- left: the Tick card --------------------------------------------------------------
cx, cy = 32, 86
s.box(cx, cy, 262, 214)
s.text(cx + 16, cy + 30, "Tick", 18, INK, weight="bold")
fields = [("symbol", "Symbol"), ("price", "Price"), ("quantity", "Quantity"), ("side", "Side"), ("timestamp", "NanoTimestamp")]
for i, (f, t) in enumerate(fields):
    s.text(cx + 16, cy + 60 + i * 24, f, 15, MUTED)
    s.text(cx + 120, cy + 60 + i * 24, t, 15, GREEN)
s.text(cx + 16, cy + 196, "each field checked when built", 14, MUTED)
# rejected value
s.box(cx, cy + 226, 262, 76, stroke=RED, sw=1.4)
s.text(cx + 16, cy + 252, "Price::new(dec!(-5))", 15, INK)
s.text(cx + 16, cy + 278, "Err: Price must be positive,", 14, RED)
s.text(cx + 16, cy + 296, "got -5  (never reaches a bar)", 14, RED)

# ---- right: chart ------------------------------------------------------------------
X0, X1, Y0, Y1 = 390, 924, 110, 380  # plot area
T0, T1 = 0, 130
P0, P1 = 3180.0, 3206.0


def px(t):
    return X0 + (t - T0) / (T1 - T0) * (X1 - X0)


def py(p):
    return Y1 - (p - P0) / (P1 - P0) * (Y1 - Y0)


s.box(312, Y0 - 24, 928 - 312, Y1 - Y0 + 70)
# buckets
for i, (a, b, lab) in enumerate([(0, 60, "bucket 15:00"), (60, 120, "bucket 15:01"), (120, 130, "15:02")]):
    fill = "#EFEADC" if i % 2 == 0 else BOX
    s.add(f'<rect x="{px(a):.1f}" y="{Y0:.1f}" width="{px(b) - px(a):.1f}" height="{Y1 - Y0:.1f}" fill="{fill}"/>')
    s.text(px(a) + 8, Y0 + 20, lab, 14, MUTED)
    s.add(f'<line x1="{px(a):.1f}" y1="{Y0}" x2="{px(a):.1f}" y2="{Y1}" stroke="{LINE}" stroke-dasharray="4 4"/>')
for p in (3185, 3195, 3205):
    s.text(X0 - 8, py(p) + 5, f"{p:,}", 13, MUTED, anchor="end")
    s.add(f'<line x1="{X0}" y1="{py(p):.1f}" x2="{X1}" y2="{py(p):.1f}" stroke="{LINE}" stroke-width="0.8"/>')
s.text(X0, Y1 + 22, "15:00:00", 13, MUTED)
s.text(px(60), Y1 + 22, "15:01:00", 13, MUTED, anchor="middle")
s.text(px(120), Y1 + 22, "15:02:00", 13, MUTED, anchor="middle")
s.text(X1, Y1 + 38, "bucket_start = ts / 60 s * 60 s", 13, MUTED, anchor="end")


def at(t):  # animation time for a tick at market second t
    return 0.6 + t / 127 * 10.4


# forming candles: running OHLC per bucket, stepped at each tick
for bucket in (0, 1, 2):
    bt = [(t, p) for t, p, _ in TICKS if t // 60 == bucket]
    if not bt:
        continue
    cxm = px(bucket * 60 + 30) if bucket < 2 else px(125)
    bw = 70 if bucket < 2 else 12
    o = bt[0][1]
    hi = lo = o
    steps_body, steps_wick = [], []
    for t, p in bt:
        hi, lo = max(hi, p), min(lo, p)
        top, bot = py(max(o, p)), py(min(o, p))
        steps_body.append((at(t), top, max(bot - top, 2)))
        steps_wick.append((at(t), py(hi), py(lo)))
    t_on = at(bt[0][0])
    closes = [at(t) for t, _, _ in TICKS if t // 60 == bucket + 1]
    color = GREEN
    g = [f'<g opacity="0">{show_between(t_on, DUR, DUR)}']
    g.append(
        f'<line x1="{cxm:.1f}" x2="{cxm:.1f}" y1="{steps_wick[0][1]:.1f}" y2="{steps_wick[0][2]:.1f}" stroke="{color}" stroke-width="2">'
        + discrete("y1", [(a, f"{b:.1f}") for a, b, _ in steps_wick], DUR, f"{steps_wick[0][1]:.1f}")
        + discrete("y2", [(a, f"{c:.1f}") for a, _, c in steps_wick], DUR, f"{steps_wick[0][2]:.1f}")
        + "</line>"
    )
    # body: hollow while forming, filled once returned
    fill_steps = [(closes[0], color)] if closes else []
    g.append(
        f'<rect x="{cxm - bw / 2:.1f}" width="{bw}" y="{steps_body[0][1]:.1f}" height="{steps_body[0][2]:.1f}" '
        f'fill="{BOX}" fill-opacity="0.9" stroke="{color}" stroke-width="2">'
        + discrete("y", [(a, f"{b:.1f}") for a, b, _ in steps_body], DUR, f"{steps_body[0][1]:.1f}")
        + discrete("height", [(a, f"{c:.1f}") for a, _, c in steps_body], DUR, f"{steps_body[0][2]:.1f}")
        + (discrete("fill", fill_steps, DUR, BOX) if fill_steps else "")
        + "</rect>"
    )
    g.append("</g>")
    s.add("".join(g))

# ticks as dots (radius by size), each appears at its time, with a short pulse
for t, p, q in TICKS:
    r = 3.5 + min(q, 4.0) * 1.6
    a = at(t)
    s.add(
        f'<circle cx="{px(t):.1f}" cy="{py(p):.1f}" r="{r:.1f}" fill="{BLUE}" fill-opacity="0.75" '
        f'stroke="{BOX}" stroke-width="1" opacity="0">{show_between(a, DUR, DUR)}</circle>'
    )

# ---- status line under chart: what push_tick returned --------------------------------
sy = 470
s.box(32, sy - 32, 896, 96)
s.text(48, sy - 8, "aggregator.push_tick(&tick)", 16, INK)
close_times = [at(61), at(127)]
states = [
    (0, close_times[0], "Ok(vec![])", MUTED, "tick absorbed: the open bar's high, low, close and volume update"),
    (close_times[0], close_times[1], "Ok(vec![bar])", GREEN,
     "15:00 bar returned: o 3186.17  h 3190.94  l 3182.16  c 3190.94  vol 10.581  12 ticks"),
    (close_times[1], DUR, "Ok(vec![bar])", GREEN,
     "15:01 bar returned: o 3192.16  h 3203.13  l 3192.16  c 3203.13  vol 10.160  11 ticks"),
]
for a, b, ret, col, note in states:
    s.add(f'<g opacity="0">{show_between(a, b, DUR)}')
    s.text(330, sy - 8, "->  " + ret, 16, col, weight="bold")
    s.text(48, sy + 22, note, 15, col if col != MUTED else INK)
    s.add("</g>")
# between the two closes, remind that ticks inside a bucket still return empty
s.text(48, sy + 48, "A tick for the next bucket closes the bar. Empty buckets in between come back as gap-fill bars with zero volume.", 13, MUTED)

# ---- bottom: indicators warm up -----------------------------------------------------
by = 560
s.text(32, by, "Finished bars feed indicators:  rsi.update(&BarInput::from(&bar))  ->  SignalValue", 16, INK)
n = 32
cw = 26.5
x = 32
for i in range(n):
    xi = x + i * (cw + 1.5)
    ready = i >= 14
    col = GREEN if ready else "#CFC8B6"
    a = 11.6 + i * 0.1
    s.add(
        f'<rect x="{xi:.1f}" y="{by + 16}" width="{cw}" height="30" rx="4" fill="{col}" opacity="0">'
        f"{show_between(a, DUR, DUR)}</rect>"
    )
s.text(32, by + 70, "bars 1 to 14: SignalValue::Unavailable", 14, MUTED)
s.text(32 + 14 * (cw + 1.5), by + 70, "bar 15 (15:14): Scalar(71.72), then a value every bar", 14, GREEN)
s.text(32, by + 96, "No NaN and no zero-fill while an indicator warms up: RSI(14) needs 15 bars, so it says so.", 14, INK)
s.text(32, by + 118, "Same types for 700+ indicators, the order book, the position ledger and the risk monitor.", 14, MUTED)

s.save(sys.argv[1])
