//! Turn a Deribit option chain into implied vols, Greeks and smile metrics.

use std::collections::BTreeMap;

use crate::black76::{self, Kind};
use crate::deribit::{parse_instrument, BookSummary};

const SECONDS_PER_YEAR: f64 = 365.0 * 86_400.0;

#[derive(Debug, Clone)]
pub struct Row {
    pub name: String,
    pub expiry: String,
    pub expiry_unix: i64,
    pub days: f64,
    pub strike: f64,
    pub kind: Kind,
    pub forward: f64,
    pub premium_usd: f64,
    pub iv: f64,              // ours, as a fraction (0.5 = 50%)
    pub deribit_iv: f64,      // Deribit's mark_iv, as a fraction
    pub otm: bool,            // out of the money (or at the money)
    pub greeks: black76::Greeks,
}

impl Row {
    /// Our IV minus Deribit's, in volatility points.
    pub fn iv_diff_points(&self) -> f64 {
        (self.iv - self.deribit_iv) * 100.0
    }
    /// log(K / F): negative for strikes below the forward.
    pub fn log_moneyness(&self) -> f64 {
        (self.strike / self.forward).ln()
    }
}

/// Price every option we can. Rows with missing data, expired options or
/// prices no volatility can explain are skipped and counted.
pub fn analyse(chain: &[BookSummary]) -> (Vec<Row>, usize) {
    let mut rows = Vec::new();
    let mut skipped = 0;
    for b in chain {
        match analyse_one(b) {
            Some(r) => rows.push(r),
            None => skipped += 1,
        }
    }
    rows.sort_by(|a, b| {
        (a.expiry_unix, a.strike as i64, a.kind == Kind::Put)
            .cmp(&(b.expiry_unix, b.strike as i64, b.kind == Kind::Put))
    });
    (rows, skipped)
}

fn analyse_one(b: &BookSummary) -> Option<Row> {
    let inst = parse_instrument(&b.instrument_name)?;
    let forward = b.underlying_price?;
    let mark_btc = b.mark_price?;
    let deribit_iv = b.mark_iv? / 100.0;
    let t = (inst.expiry_unix as f64 - b.creation_timestamp as f64 / 1000.0) / SECONDS_PER_YEAR;
    if t <= 0.0 || forward <= 0.0 {
        return None;
    }
    // Deribit quotes BTC options in BTC; the USD premium is price x forward.
    let premium_usd = mark_btc * forward;
    let iv = black76::implied_vol(inst.kind, premium_usd, forward, inst.strike, t)?;
    let otm = match inst.kind {
        Kind::Call => inst.strike >= forward,
        Kind::Put => inst.strike < forward,
    };
    Some(Row {
        name: b.instrument_name.clone(),
        expiry: inst.expiry_code,
        expiry_unix: inst.expiry_unix,
        days: t * 365.0,
        strike: inst.strike,
        kind: inst.kind,
        forward,
        premium_usd,
        iv,
        deribit_iv,
        otm,
        greeks: black76::greeks(inst.kind, forward, inst.strike, t, iv),
    })
}

#[derive(Debug, Clone, Copy)]
pub struct Agreement {
    pub n: usize,
    pub median: f64,
    pub p95: f64,
    pub max: f64,
}

/// How closely our IVs match Deribit's, in absolute volatility points.
pub fn agreement<'a>(rows: impl Iterator<Item = &'a Row>) -> Option<Agreement> {
    let mut d: Vec<f64> = rows.map(|r| r.iv_diff_points().abs()).collect();
    if d.is_empty() {
        return None;
    }
    d.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f64| d[((d.len() - 1) as f64 * q).round() as usize];
    Some(Agreement { n: d.len(), median: at(0.5), p95: at(0.95), max: *d.last().unwrap() })
}

/// Per-expiry smile summary built from out-of-the-money options only.
#[derive(Debug, Clone)]
pub struct Smile {
    pub expiry: String,
    pub days: f64,
    pub forward: f64,
    pub atm_iv: f64,
    /// IV of the ~25-delta call minus the ~25-delta put. Negative = puts richer
    /// (market paying up for downside protection).
    pub risk_reversal_25d: Option<f64>,
    /// Average of the 25-delta wings minus ATM: how "smiley" the curve is.
    pub butterfly_25d: Option<f64>,
    pub options: usize,
}

pub fn smiles(rows: &[Row]) -> Vec<Smile> {
    let mut by_expiry: BTreeMap<i64, Vec<&Row>> = BTreeMap::new();
    for r in rows.iter().filter(|r| r.otm) {
        by_expiry.entry(r.expiry_unix).or_default().push(r);
    }
    by_expiry
        .into_values()
        .filter_map(|v| {
            let first = v[0];
            let atm = v.iter().min_by(|a, b| {
                a.log_moneyness().abs().total_cmp(&b.log_moneyness().abs())
            })?;
            let nearest_delta = |kind: Kind, target: f64| {
                v.iter()
                    .filter(|r| r.kind == kind)
                    .min_by(|a, b| {
                        (a.greeks.delta - target).abs().total_cmp(&(b.greeks.delta - target).abs())
                    })
                    .filter(|r| (r.greeks.delta - target).abs() < 0.08)
                    .map(|r| r.iv)
            };
            let call25 = nearest_delta(Kind::Call, 0.25);
            let put25 = nearest_delta(Kind::Put, -0.25);
            let (rr, bf) = match (call25, put25) {
                (Some(c), Some(p)) => (Some(c - p), Some(0.5 * (c + p) - atm.iv)),
                _ => (None, None),
            };
            Some(Smile {
                expiry: first.expiry.clone(),
                days: first.days,
                forward: first.forward,
                atm_iv: atm.iv,
                risk_reversal_25d: rr,
                butterfly_25d: bf,
                options: v.len(),
            })
        })
        .collect()
}
