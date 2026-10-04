//! Property tests for the normal distribution, timeframe bucketing, the order book
//! and bar aggregation: invariants that must hold for any input, not just the
//! hand-picked cases in the unit tests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fin_primitives::normal;
use fin_primitives::options::greeks::{bsm_price_q, implied_vol, BSMInputs, OptionType};
use fin_primitives::ohlcv::{OhlcvAggregator, Timeframe};
use fin_primitives::orderbook::{BookDelta, DeltaAction, OrderBook};
use fin_primitives::tick::Tick;
use fin_primitives::types::{NanoTimestamp, Price, Quantity, Side, Symbol};
use proptest::prelude::*;
use rust_decimal::Decimal;

fn timeframe() -> impl Strategy<Value = Timeframe> {
    prop_oneof![
        (1u32..=3_600).prop_map(Timeframe::Seconds),
        (1u32..=1_440).prop_map(Timeframe::Minutes),
        (1u32..=48).prop_map(Timeframe::Hours),
        (1u32..=30).prop_map(Timeframe::Days),
        (1u32..=8).prop_map(Timeframe::Weeks),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    #[test]
    fn cdf_is_monotone_and_bounded(a in -40.0f64..40.0, b in -40.0f64..40.0) {
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        let (cl, ch) = (normal::cdf(lo), normal::cdf(hi));
        prop_assert!((0.0..=1.0).contains(&cl));
        prop_assert!((0.0..=1.0).contains(&ch));
        prop_assert!(cl <= ch, "cdf({lo}) = {cl} > cdf({hi}) = {ch}");
    }

    #[test]
    fn inv_cdf_inverts_cdf(p in 1e-12f64..(1.0 - 1e-12)) {
        let z = normal::inv_cdf(p);
        prop_assert!(z.is_finite());
        let back = normal::cdf(z);
        prop_assert!((back - p).abs() <= 1e-13 + 1e-9 * p, "p={p} z={z} back={back}");
    }

    /// Implied vol recovers the vol that produced the price, for calls and puts from
    /// deep in to deep out of the money, wherever the price carries enough
    /// information (vega not vanishingly small).
    #[test]
    fn implied_vol_inverts_the_price(
        k in 50.0f64..200.0, sigma in 0.05f64..1.5, t in 0.02f64..3.0, r in -0.01f64..0.08, call in any::<bool>()
    ) {
        let inp = BSMInputs { S: 100.0, K: k, r, q: 0.0, sigma, T: t };
        let ty = if call { OptionType::Call } else { OptionType::Put };
        let price = bsm_price_q(&inp, ty);
        let vega = fin_primitives::options::greeks::vega(&inp) * 100.0;
        prop_assume!(price > 1e-8 && vega > 1e-4);
        let iv = implied_vol(price, &BSMInputs { sigma: 0.3, ..inp }, ty);
        prop_assert!(iv.is_some(), "no solution for price {price}");
        let iv = iv.unwrap();
        prop_assert!((iv - sigma).abs() < 1e-6, "sigma {sigma} recovered as {iv} (price {price}, vega {vega})");
    }

    #[test]
    fn bucket_start_contains_the_timestamp(tf in timeframe(), ts in -4_000_000_000_000_000_000i64..4_000_000_000_000_000_000) {
        let len = tf.to_nanos().unwrap();
        let start = tf.bucket_start(NanoTimestamp::new(ts)).unwrap().nanos();
        prop_assert!(start <= ts && ts - start < len, "ts={ts} start={start} len={len}");
        // Idempotent: the start of a bucket is in its own bucket.
        prop_assert_eq!(tf.bucket_start(NanoTimestamp::new(start)).unwrap().nanos(), start);
    }

    /// Any sequence of deltas, valid or not, leaves the book uncrossed and its
    /// sequence number equal to the number of accepted deltas. Nothing panics.
    #[test]
    fn order_book_never_crosses(
        ops in prop::collection::vec((any::<bool>(), 1u32..200, 0u32..50, any::<bool>(), 0u64..3), 1..300)
    ) {
        let mut book = OrderBook::new(Symbol::new("BTC-USD").unwrap());
        let mut accepted = 0u64;
        for (bid, px, qty, remove, seq_skew) in ops {
            let delta = BookDelta {
                side: if bid { Side::Bid } else { Side::Ask },
                price: Price::new(Decimal::from(px)).unwrap(),
                quantity: Quantity::new(Decimal::from(qty)).unwrap(),
                action: if remove { DeltaAction::Remove } else { DeltaAction::Set },
                // Mostly the right sequence number, sometimes a gap.
                sequence: book.sequence() + 1 + if seq_skew == 2 { 1 } else { 0 },
            };
            if book.apply_delta(delta).is_ok() {
                accepted += 1;
            }
            if let (Some(b), Some(a)) = (book.best_bid(), book.best_ask()) {
                prop_assert!(b.price < a.price, "crossed book: bid {} ask {}", b.price.value(), a.price.value());
            }
            prop_assert_eq!(book.sequence(), accepted);
        }
    }

    /// Bars come out in time order, never overlap, and conserve volume, even when
    /// some ticks arrive late.
    #[test]
    fn bars_are_ordered_and_conserve_volume(
        steps in prop::collection::vec((-30_000i64..120_000, 1u32..1_000, 1u32..100), 1..200)
    ) {
        let mut agg = OhlcvAggregator::new(Symbol::new("X").unwrap(), Timeframe::Minutes(1)).unwrap();
        let mut ts = 1_700_000_000_000_000_000i64;
        let mut bars = Vec::new();
        let mut fed = Decimal::ZERO;
        for (dt_ms, px, qty) in steps {
            ts += dt_ms * 1_000_000; // negative steps make late ticks
            let tick = Tick::new(
                Symbol::new("X").unwrap(),
                Price::new(Decimal::from(px)).unwrap(),
                Quantity::new(Decimal::from(qty)).unwrap(),
                Side::Bid,
                NanoTimestamp::new(ts),
            );
            let late_before = agg.late_tick_count();
            bars.extend(agg.push_tick(&tick).unwrap());
            if agg.late_tick_count() == late_before {
                fed += Decimal::from(qty);
            }
        }
        bars.extend(agg.flush());
        let total: Decimal = bars.iter().map(|b| b.volume.value()).sum();
        prop_assert_eq!(total, fed);
        for w in bars.windows(2) {
            prop_assert!(w[0].ts_open < w[1].ts_open, "bars out of order");
        }
        for b in &bars {
            prop_assert!(b.low.value() <= b.high.value());
        }
    }
}
