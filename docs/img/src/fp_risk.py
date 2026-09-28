"""fin-primitives: fills and marks -> PositionLedger -> net_liquidation_value -> RiskMonitor -> Vec<RiskBreach>.

Every number is from `cargo run --example position_risk`, run 2026-09-28.
"""
import sys
from common import *

EVENTS = [
    ("buy  200 AAPL @ 175.00", 99999.00, 0.00, []),
    ("buy   80 MSFT @ 410.00", 99998.00, 0.00, []),
    ("mark AAPL 178.40", 100678.00, 0.00, []),
    ("mark MSFT 415.25", 101098.00, 0.00, []),
    ("sell 100 AAPL @ 179.10", 101237.00, 0.00, []),
    ("mark MSFT 398.10", 99865.00, 1.36, []),
    ("mark AAPL 166.80", 98635.00, 2.57, []),
    ("mark MSFT 371.50", 96507.00, 4.67, ["max_drawdown: drawdown 4.67% > 4.00%", "min_equity: equity 96507.00 < floor 96600"]),
    ("sell  80 MSFT @ 372.00", 96546.00, 4.63, ["max_drawdown: drawdown 4.63% > 4.00%", "min_equity: equity 96546.00 < floor 96600"]),
    ("mark AAPL 171.35", 97001.00, 4.18, ["max_drawdown: drawdown 4.18% > 4.00%"]),
]
DUR = 15.0
STEP = 1.1
W, H = 960, 560

s = Svg(W, H, "Animated diagram of a risk check in fin-primitives, using the real session from the position_risk "
        "example. Fills and price marks go into a PositionLedger; its net_liquidation_value (cash plus the market "
        "value of open positions) goes into RiskMonitor::update, which runs a MaxDrawdownRule of 4 percent and a "
        "MinEquityRule floor of 96,600 and returns a Vec of RiskBreach. Equity peaks at 101,237 after event 5; "
        "at event 8, marking MSFT at 371.50, equity is 96,507, drawdown 4.67 percent, and both rules fire.")

s.text(32, 44, "How a risk check works", 24, font=SERIF)
s.text(32, 70, "the real session from cargo run --example position_risk, one event at a time", 14, MUTED)

# pipeline row
y = 96
boxes = [
    (32, 170, "Fill / mark", "qty, price, fee", INK),
    (232, 190, "PositionLedger", "cash, avg cost, P&L", INK),
    (452, 200, "RiskMonitor", "::update(equity)", AMBER),
    (682, 246, "Vec<RiskBreach>", "empty = all rules pass", RED),
]
for x, w, t1, t2, col in boxes:
    s.box(x, y, w, 62, stroke=col if col != INK else LINE, sw=1.6)
    s.text(x + w / 2, y + 27, t1, 16, col, anchor="middle", weight="bold")
    s.text(x + w / 2, y + 49, t2, 14, MUTED, anchor="middle")
s.arrow(202, y + 31, 230, y + 31)
s.arrow(422, y + 31, 450, y + 31)
s.arrow(652, y + 31, 680, y + 31)
s.text(437, y + 84, "net_liquidation_value(&marks) = cash + market value of open positions", 14, MUTED, anchor="middle")
s.text(437, y + 106, "rules:  MaxDrawdownRule { threshold_pct: 4 }   MinEquityRule { floor: 96,600 }", 14, AMBER, anchor="middle")

# pulse moving along the pipeline on each event
for i in range(len(EVENTS)):
    t0 = 0.6 + i * STEP
    s.add(
        f'<circle r="7" fill="{BLUE}" opacity="0">{show_between(t0, t0 + 0.9, DUR)}'
        f'<animateMotion dur="{DUR}s" repeatCount="indefinite" calcMode="linear" keyPoints="0;0;1;1" '
        f'keyTimes="0;{t0 / DUR:.4f};{(t0 + 0.9) / DUR:.4f};1" path="M117,{y + 31} L567,{y + 31} L805,{y + 31}"/></circle>'
    )

# chart
X0, X1, Y0, Y1 = 120, 620, 250, 500
E0, E1 = 96000, 101600


def cx(i):
    return X0 + 25 + i * (X1 - X0 - 50) / 9


def cy(e):
    return Y1 - (e - E0) / (E1 - E0) * (Y1 - Y0)


s.box(32, Y0 - 24, 610, Y1 - Y0 + 66)
for e in (97000, 99000, 101000):
    s.add(f'<line x1="{X0}" x2="{X1}" y1="{cy(e):.1f}" y2="{cy(e):.1f}" stroke="{LINE}" stroke-width="0.8"/>')
    s.text(X0 - 8, cy(e) + 5, f"{e:,}", 13, MUTED, anchor="end")
peak = 101237.0
dd_line = peak * 0.96
lines = [(peak, "peak 101,237", MUTED, "5 4"), (dd_line, "4% below peak", AMBER, "6 4"), (96600, "floor 96,600", RED, "6 4")]
for v, lab, col, dash in lines:
    s.add(f'<line x1="{X0}" x2="{X1}" y1="{cy(v):.1f}" y2="{cy(v):.1f}" stroke="{col}" stroke-width="1.5" stroke-dasharray="{dash}"/>')
    if v == 96600:  # left end: the right end is where events 8 to 10 sit
        s.text(X0 + 4, cy(v) - 6, lab, 13, col)
    else:
        s.text(X1 - 4, cy(v) - 6, lab, 13, col, anchor="end")
for i in range(10):
    s.text(cx(i), Y1 + 22, str(i + 1), 13, MUTED, anchor="middle")
s.text(X0 - 8, Y1 + 22, "event", 13, MUTED, anchor="end")

# equity segments and points appear one by one
for i, (label, eq, dd, br) in enumerate(EVENTS):
    t0 = 0.6 + i * STEP + 0.9
    col = RED if br else GREEN
    if i:
        pe = EVENTS[i - 1][1]
        s.add(f'<line x1="{cx(i - 1):.1f}" y1="{cy(pe):.1f}" x2="{cx(i):.1f}" y2="{cy(eq):.1f}" stroke="{INK}" '
              f'stroke-width="2" opacity="0">{show_between(t0, DUR, DUR)}</line>')
    s.add(f'<circle cx="{cx(i):.1f}" cy="{cy(eq):.1f}" r="6" fill="{col}" stroke="{BOX}" stroke-width="1.5" opacity="0">'
          f'{show_between(t0, DUR, DUR)}</circle>')

# right panel: the current event, as the example prints it
PX = 660
s.box(PX, Y0 - 24, 268, Y1 - Y0 + 66)
s.text(PX + 16, Y0 + 2, "event", 14, MUTED)
for i, (label, eq, dd, br) in enumerate(EVENTS):
    t0 = 0.6 + i * STEP + 0.9
    t1 = 0.6 + (i + 1) * STEP + 0.9 if i < 9 else DUR
    s.add(f'<g opacity="0">{show_between(t0, t1, DUR)}')
    s.text(PX + 16, Y0 + 28, f"#{i + 1}  {label.split('  ')[0] if False else label}", 15, INK)
    s.text(PX + 16, Y0 + 62, "equity", 14, MUTED)
    s.text(PX + 250, Y0 + 62, f"{eq:,.2f}", 16, INK, anchor="end")
    s.text(PX + 16, Y0 + 88, "drawdown", 14, MUTED)
    s.text(PX + 250, Y0 + 88, f"{dd:.2f}%", 16, RED if dd > 4 else INK, anchor="end")
    if br:
        s.text(PX + 16, Y0 + 124, "BREACH", 18, RED, weight="bold")
        for j, b in enumerate(br):
            name, detail = b.split(": ", 1)
            s.text(PX + 16, Y0 + 152 + j * 44, name, 14, RED)
            s.text(PX + 16, Y0 + 170 + j * 44, detail, 13, INK)
    else:
        s.text(PX + 16, Y0 + 124, "ok", 18, GREEN, weight="bold")
        s.text(PX + 16, Y0 + 152, "no rule fired, empty Vec", 14, MUTED)
    s.add("</g>")

s.save(sys.argv[1])
