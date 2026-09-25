//! Small formatting helpers shared by the examples: fixed-width decimals,
//! thousands separators, depth bars and ANSI colors that respect `NO_COLOR`.
//! Nothing here is part of the library.

#![allow(dead_code)]

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

/// A horizontal bar using eighth-block glyphs for sub-cell precision.
pub fn bar(value: Decimal, max: Decimal, width: usize) -> String {
    if max.is_zero() {
        return String::new();
    }
    let eighths = (value / max * Decimal::from(width * 8))
        .round()
        .to_usize()
        .unwrap_or(0);
    let full = eighths / 8;
    let part = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉"][eighths % 8];
    let mut s = "█".repeat(full);
    if eighths % 8 != 0 {
        s.push_str(part);
    }
    s
}

/// Fixed decimal places, keeping trailing zeros so columns line up.
pub fn fixed(d: Decimal, dp: u32) -> String {
    let mut r = d.round_dp(dp);
    r.rescale(dp);
    r.to_string()
}

/// Fixed decimal places with thousands separators.
pub fn money(d: Decimal, dp: u32) -> String {
    let s = fixed(d, dp);
    let (int, frac) = s.split_once('.').unwrap_or((&s, ""));
    let mut out = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if !frac.is_empty() {
        out.push('.');
        out.push_str(frac);
    }
    out
}

/// Minimal ANSI styling. Helpers return a `Styled` value so `{:>11}` pads by
/// the visible text, not by the escape codes around it.
pub struct Paint {
    on: bool,
}

impl Paint {
    pub fn detect() -> Self {
        use std::io::IsTerminal;
        let forced = std::env::var_os("FORCE_COLOR").is_some();
        let disabled = std::env::var_os("NO_COLOR").is_some();
        Self {
            on: forced || (!disabled && std::io::stdout().is_terminal()),
        }
    }
    pub fn wrap(&self, code: &str, s: &str) -> Styled {
        Styled {
            text: s.to_owned(),
            code: if self.on { Some(code.to_owned()) } else { None },
        }
    }
    pub fn red(&self, s: &str) -> Styled {
        self.wrap("38;5;203", s)
    }
    pub fn green(&self, s: &str) -> Styled {
        self.wrap("38;5;78", s)
    }
    pub fn dim(&self, s: &str) -> Styled {
        self.wrap("38;5;245", s)
    }
    #[allow(clippy::unused_self)]
    pub fn plain(&self, s: &str) -> Styled {
        Styled {
            text: s.to_owned(),
            code: None,
        }
    }
    pub fn yellow(&self, s: &str) -> Styled {
        self.wrap("38;5;179", s)
    }
    pub fn bold(&self, s: &str) -> Styled {
        self.wrap("1", s)
    }
}

/// Text plus an optional ANSI code. Its `Display` honours width and alignment
/// on the visible text, so styled cells still line up in columns.
pub struct Styled {
    text: String,
    code: Option<String>,
}

impl std::fmt::Display for Styled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let width = f.width().unwrap_or(0);
        let len = self.text.chars().count();
        let pad = width.saturating_sub(len);
        let (left, right) = match f.align() {
            Some(std::fmt::Alignment::Right) => (pad, 0),
            Some(std::fmt::Alignment::Center) => (pad / 2, pad - pad / 2),
            _ => (0, pad),
        };
        write!(f, "{}", " ".repeat(left))?;
        match &self.code {
            Some(code) => write!(f, "\x1b[{code}m{}\x1b[0m", self.text)?,
            None => write!(f, "{}", self.text)?,
        }
        write!(f, "{}", " ".repeat(right))
    }
}
