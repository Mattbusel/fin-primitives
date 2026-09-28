"""Shared helpers for the hand-built explainer SVGs (cream card, works on light and dark GitHub)."""
from xml.sax.saxutils import escape

BG = "#F7F4EC"
BOX = "#FFFDF8"
INK = "#16181D"
MUTED = "#6F6A5E"
LINE = "#D9D2C1"
GREEN = "#1E7F57"
RED = "#C4432B"
AMBER = "#9C7A22"
BLUE = "#2F5E9E"
SERIF = "Georgia,'Iowan Old Style',ui-serif,serif"
MONO = "'Cascadia Mono','SF Mono',ui-monospace,Menlo,Consolas,monospace"


class Svg:
    def __init__(self, w, h, label):
        self.w, self.h = w, h
        self.parts = [
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" '
            f'role="img" aria-label="{escape(label, {chr(34): "&quot;"})}">',
            f'<rect width="{w}" height="{h}" rx="14" fill="{BG}"/>',
        ]

    def add(self, s):
        self.parts.append(s)

    def text(self, x, y, s, size=14, fill=INK, font=MONO, anchor="start", weight=None, extra="", raw=False):
        w = f' font-weight="{weight}"' if weight else ""
        body = s if raw else escape(s)
        self.add(
            f'<text x="{x:.1f}" y="{y:.1f}" font-family="{font}" font-size="{size}" fill="{fill}" '
            f'text-anchor="{anchor}"{w}{extra}>{body}</text>'
        )

    def box(self, x, y, w, h, stroke=LINE, sw=1.2, fill=BOX, rx=8, extra=""):
        self.add(
            f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" rx="{rx}" fill="{fill}" '
            f'stroke="{stroke}" stroke-width="{sw}"{extra}/>'
        )

    def arrow(self, x1, y1, x2, y2, color=MUTED, sw=1.6):
        import math

        ang = math.atan2(y2 - y1, x2 - x1)
        hx, hy = x2 - 9 * math.cos(ang), y2 - 9 * math.sin(ang)
        px, py = -math.sin(ang) * 5, math.cos(ang) * 5
        self.add(f'<line x1="{x1:.1f}" y1="{y1:.1f}" x2="{hx:.1f}" y2="{hy:.1f}" stroke="{color}" stroke-width="{sw}"/>')
        self.add(
            f'<path d="M{x2:.1f},{y2:.1f} L{hx + px:.1f},{hy + py:.1f} L{hx - px:.1f},{hy - py:.1f} Z" fill="{color}"/>'
        )

    def save(self, path):
        self.parts.append("</svg>")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write("\n".join(self.parts) + "\n")


def show_between(t_on, t_off, dur):
    """opacity animation: hidden, visible from t_on to t_off (seconds), looping every dur."""
    a, b = t_on / dur, min(t_off / dur, 1.0)
    eps = 0.0005
    if b >= 1.0:
        vals, kt = "0;0;1;1", f"0;{a:.4f};{a + eps:.4f};1"
    else:
        vals, kt = "0;0;1;1;0;0", f"0;{a:.4f};{a + eps:.4f};{b:.4f};{b + eps:.4f};1"
    return (
        f'<animate attributeName="opacity" values="{vals}" keyTimes="{kt}" dur="{dur}s" '
        f'repeatCount="indefinite"/>'
    )


def discrete(attr, times_vals, dur, initial):
    """step animation of one attribute: list of (t_seconds, value), starts at initial."""
    kts, vals = ["0"], [str(initial)]
    for t, v in times_vals:
        kts.append(f"{t / dur:.4f}")
        vals.append(str(v))
    return (
        f'<animate attributeName="{attr}" calcMode="discrete" values="{";".join(vals)}" '
        f'keyTimes="{";".join(kts)}" dur="{dur}s" repeatCount="indefinite"/>'
    )
