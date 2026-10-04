//! Apache Arrow export and import for bars (`arrow` feature, needs Rust 1.88+).
//!
//! [`bars_to_record_batch`] turns a slice of [`OhlcvBar`] into an Arrow
//! `RecordBatch` from the `arrow-array` crate: DataFusion takes it directly, and
//! Polars, DuckDB or `pyarrow` can read it through Arrow IPC files or the Arrow C
//! data interface, with no CSV round trip. Prices and volume stay exact: they are written as
//! `Decimal128(38, scale)` columns, not floats. [`record_batch_to_bars`] reads
//! such a batch back and re-validates every value.
//!
//! | column | Arrow type |
//! |---|---|
//! | `symbol` | `Utf8` |
//! | `ts_open`, `ts_close` | `Timestamp(Nanosecond, "UTC")` |
//! | `open`, `high`, `low`, `close`, `volume` | `Decimal128(38, s)`, `s` = the largest scale in that column |
//! | `tick_count` | `UInt64` |
//!
//! ```
//! # #[cfg(feature = "arrow")] {
//! use fin_primitives::arrow::{bars_to_record_batch, record_batch_to_bars};
//! use fin_primitives::ohlcv::OhlcvBar;
//! use fin_primitives::types::{NanoTimestamp, Price, Quantity, Symbol};
//! use rust_decimal_macros::dec;
//!
//! let bar = OhlcvBar {
//!     symbol: Symbol::new("BTC-USD").unwrap(),
//!     open: Price::new(dec!(64250.50)).unwrap(),
//!     high: Price::new(dec!(64262.00)).unwrap(),
//!     low: Price::new(dec!(64240.25)).unwrap(),
//!     close: Price::new(dec!(64255.75)).unwrap(),
//!     volume: Quantity::new(dec!(12.345)).unwrap(),
//!     ts_open: NanoTimestamp::new(1_767_625_200_000_000_000),
//!     ts_close: NanoTimestamp::new(1_767_625_259_000_000_000),
//!     tick_count: 42,
//! };
//! let batch = bars_to_record_batch(&[bar.clone()]).unwrap();
//! assert_eq!(batch.num_rows(), 1);
//! assert_eq!(record_batch_to_bars(&batch).unwrap(), vec![bar]);
//! # }
//! ```

use crate::error::FinError;
use crate::ohlcv::OhlcvBar;
use crate::types::{NanoTimestamp, Price, Quantity, Symbol};
use arrow_array::cast::AsArray;
use arrow_array::Array;
use arrow_array::types::{Decimal128Type, TimestampNanosecondType, UInt64Type};
use arrow_array::{
    ArrayRef, Decimal128Array, RecordBatch, StringArray, TimestampNanosecondArray, UInt64Array,
};
use arrow_schema::{DataType, Field, Schema, TimeUnit};
use rust_decimal::Decimal;
use std::sync::Arc;

/// Arrow's largest Decimal128 precision.
const PRECISION: u8 = 38;

fn err(e: impl std::fmt::Display) -> FinError {
    FinError::InvalidInput(format!("arrow: {e}"))
}

/// One decimal column at a common scale.
fn decimal_column(values: &[Decimal]) -> Result<(ArrayRef, u8), FinError> {
    let scale = values.iter().map(Decimal::scale).max().unwrap_or(0);
    let limit = 10i128.pow(u32::from(PRECISION));
    let mut out = Vec::with_capacity(values.len());
    for d in values {
        let factor = 10i128.pow(scale - d.scale());
        let v = d
            .mantissa()
            .checked_mul(factor)
            .filter(|v| v.abs() < limit)
            .ok_or_else(|| err(format!("{d} does not fit Decimal128({PRECISION}, {scale})")))?;
        out.push(v);
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let scale_i8 = scale as i8; // rust_decimal scales are 0..=28
    let arr = Decimal128Array::from(out).with_precision_and_scale(PRECISION, scale_i8).map_err(err)?;
    Ok((Arc::new(arr), scale as u8))
}

fn ts_type() -> DataType {
    DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into()))
}

/// Convert bars to an Arrow `RecordBatch` (schema in the [module docs](self)).
///
/// # Errors
/// [`FinError::InvalidInput`] if a value does not fit `Decimal128(38, scale)` at the
/// column's common scale (only possible with very large numbers carrying many decimals).
pub fn bars_to_record_batch(bars: &[OhlcvBar]) -> Result<RecordBatch, FinError> {
    let col = |f: fn(&OhlcvBar) -> Decimal| bars.iter().map(f).collect::<Vec<_>>();
    let (open, so) = decimal_column(&col(|b| b.open.value()))?;
    let (high, sh) = decimal_column(&col(|b| b.high.value()))?;
    let (low, sl) = decimal_column(&col(|b| b.low.value()))?;
    let (close, sc) = decimal_column(&col(|b| b.close.value()))?;
    let (volume, sv) = decimal_column(&col(|b| b.volume.value()))?;
    #[allow(clippy::cast_possible_wrap)]
    let dec = |s: u8| DataType::Decimal128(PRECISION, s as i8);
    let schema = Schema::new(vec![
        Field::new("symbol", DataType::Utf8, false),
        Field::new("ts_open", ts_type(), false),
        Field::new("ts_close", ts_type(), false),
        Field::new("open", dec(so), false),
        Field::new("high", dec(sh), false),
        Field::new("low", dec(sl), false),
        Field::new("close", dec(sc), false),
        Field::new("volume", dec(sv), false),
        Field::new("tick_count", DataType::UInt64, false),
    ]);
    let columns: Vec<ArrayRef> = vec![
        Arc::new(StringArray::from_iter_values(bars.iter().map(|b| b.symbol.as_str()))),
        Arc::new(TimestampNanosecondArray::from_iter_values(bars.iter().map(|b| b.ts_open.nanos())).with_timezone("UTC")),
        Arc::new(TimestampNanosecondArray::from_iter_values(bars.iter().map(|b| b.ts_close.nanos())).with_timezone("UTC")),
        open,
        high,
        low,
        close,
        volume,
        Arc::new(UInt64Array::from_iter_values(bars.iter().map(|b| b.tick_count))),
    ];
    RecordBatch::try_new(Arc::new(schema), columns).map_err(err)
}

fn column<'a>(batch: &'a RecordBatch, name: &str) -> Result<&'a ArrayRef, FinError> {
    batch.column_by_name(name).ok_or_else(|| err(format!("missing column `{name}`")))
}

fn decimals(batch: &RecordBatch, name: &str) -> Result<Vec<Decimal>, FinError> {
    let col = column(batch, name)?;
    let DataType::Decimal128(_, scale) = col.data_type() else {
        return Err(err(format!("column `{name}` is {}, expected Decimal128", col.data_type())));
    };
    let scale = u32::try_from(*scale).map_err(|_| err(format!("negative scale in `{name}`")))?;
    let arr = col.as_primitive_opt::<Decimal128Type>().ok_or_else(|| err(format!("column `{name}` is not Decimal128")))?;
    arr.iter()
        .map(|v| {
            let v = v.ok_or_else(|| err(format!("null in `{name}`")))?;
            Decimal::try_from_i128_with_scale(v, scale).map_err(err)
        })
        .collect()
}

/// Read bars back from a batch with the schema written by [`bars_to_record_batch`]
/// (columns are found by name; extra columns are ignored).
///
/// # Errors
/// [`FinError::InvalidInput`] for a missing or mistyped column, a null, or a value
/// outside `rust_decimal`'s range; the usual [`Price`]/[`Quantity`]/[`Symbol`]
/// errors for values that fail validation (for example a zero price).
pub fn record_batch_to_bars(batch: &RecordBatch) -> Result<Vec<OhlcvBar>, FinError> {
    let sym = column(batch, "symbol")?.as_string_opt::<i32>().ok_or_else(|| err("`symbol` is not Utf8"))?;
    let ts = |name: &str| -> Result<Vec<i64>, FinError> {
        let c = column(batch, name)?
            .as_primitive_opt::<TimestampNanosecondType>()
            .ok_or_else(|| err(format!("`{name}` is not Timestamp(Nanosecond)")))?;
        c.iter().map(|v| v.ok_or_else(|| err(format!("null in `{name}`")))).collect()
    };
    let (ts_open, ts_close) = (ts("ts_open")?, ts("ts_close")?);
    let (open, high, low, close, volume) = (
        decimals(batch, "open")?,
        decimals(batch, "high")?,
        decimals(batch, "low")?,
        decimals(batch, "close")?,
        decimals(batch, "volume")?,
    );
    let ticks = column(batch, "tick_count")?
        .as_primitive_opt::<UInt64Type>()
        .ok_or_else(|| err("`tick_count` is not UInt64"))?;
    (0..batch.num_rows())
        .map(|i| {
            let symbol = sym.is_valid(i).then(|| sym.value(i)).ok_or_else(|| err("null in `symbol`"))?;
            Ok(OhlcvBar {
                symbol: Symbol::new(symbol)?,
                open: Price::new(open[i])?,
                high: Price::new(high[i])?,
                low: Price::new(low[i])?,
                close: Price::new(close[i])?,
                volume: Quantity::new(volume[i])?,
                ts_open: NanoTimestamp::new(ts_open[i]),
                ts_close: NanoTimestamp::new(ts_close[i]),
                tick_count: ticks.is_valid(i).then(|| ticks.value(i)).ok_or_else(|| err("null in `tick_count`"))?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn bar(sym: &str, o: Decimal, c: Decimal, v: Decimal, t: i64) -> OhlcvBar {
        OhlcvBar {
            symbol: Symbol::new(sym).unwrap(),
            open: Price::new(o).unwrap(),
            high: Price::new(o.max(c) + dec!(1)).unwrap(),
            low: Price::new(o.min(c) / dec!(2)).unwrap(),
            close: Price::new(c).unwrap(),
            volume: Quantity::new(v).unwrap(),
            ts_open: NanoTimestamp::new(t),
            ts_close: NanoTimestamp::new(t + 59_999_999_999),
            tick_count: 7,
        }
    }

    #[test]
    fn round_trip_keeps_every_digit_across_mixed_scales() {
        let bars = vec![
            bar("BTC-USD", dec!(64250.5), dec!(64251), dec!(0.00012345), 0),
            bar("ETH-USD", dec!(3186.17), dec!(3190.9412345678), dec!(1200), 60_000_000_000),
            bar("SHIB-USD", dec!(0.000012345678901234), dec!(0.000012345), dec!(98765432109876543210), -60_000_000_000),
        ];
        let batch = bars_to_record_batch(&bars).unwrap();
        assert_eq!(batch.num_rows(), 3);
        let back = record_batch_to_bars(&batch).unwrap();
        for (a, b) in bars.iter().zip(&back) {
            assert_eq!(a.symbol, b.symbol);
            assert_eq!(a.ts_open, b.ts_open);
            assert_eq!(a.tick_count, b.tick_count);
            // Same numbers; the scale may grow to the column's common scale.
            for (x, y) in [(a.open, b.open), (a.high, b.high), (a.low, b.low), (a.close, b.close)] {
                assert_eq!(x.value(), y.value());
            }
            assert_eq!(a.volume.value(), b.volume.value());
        }
    }

    #[test]
    fn schema_uses_exact_decimal_and_utc_timestamps() {
        let batch = bars_to_record_batch(&[bar("X", dec!(1.25), dec!(1.5), dec!(3), 0)]).unwrap();
        let s = batch.schema();
        // Each column takes the largest scale of its own values: 1.25 -> 2, 1.5 -> 1.
        assert_eq!(s.field_with_name("open").unwrap().data_type(), &DataType::Decimal128(38, 2));
        assert_eq!(s.field_with_name("close").unwrap().data_type(), &DataType::Decimal128(38, 1));
        assert_eq!(s.field_with_name("ts_open").unwrap().data_type(), &ts_type());
    }

    #[test]
    fn empty_input_gives_an_empty_batch() {
        let batch = bars_to_record_batch(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert!(record_batch_to_bars(&batch).unwrap().is_empty());
    }

    #[test]
    fn invalid_rows_are_rejected_on_import() {
        // A zero close price must not come back as an OhlcvBar.
        let good = bars_to_record_batch(&[bar("X", dec!(1), dec!(2), dec!(1), 0)]).unwrap();
        let mut cols: Vec<ArrayRef> = good.columns().to_vec();
        let idx = good.schema().index_of("close").unwrap();
        cols[idx] = Arc::new(Decimal128Array::from(vec![0i128]).with_precision_and_scale(38, 0).unwrap());
        let mut fields: Vec<Field> = good.schema().fields().iter().map(|f| f.as_ref().clone()).collect();
        fields[idx] = Field::new("close", DataType::Decimal128(38, 0), false);
        let bad = RecordBatch::try_new(Arc::new(Schema::new(fields)), cols).unwrap();
        assert!(matches!(record_batch_to_bars(&bad), Err(FinError::InvalidPrice(_))));
    }
}
