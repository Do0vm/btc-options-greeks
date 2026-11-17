//! Black-76: the Black-Scholes model written in terms of the forward price.
//!
//! Deribit BTC options settle against a futures-style forward and quote a zero
//! interest rate, so no discounting is applied. All prices here are in USD.

use std::f64::consts::{PI, SQRT_2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Call,
    Put,
}

/// Complementary error function, accurate to about 1e-12 relative error.
///
/// `std` has no `erf`, so: a positive-term series for small |x| (no
/// cancellation) and a continued fraction for the tail, where 1 - erf(x)
/// would lose all its precision.
pub fn erfc(x: f64) -> f64 {
    if x < 0.0 {
        return 2.0 - erfc(-x);
    }
    if x < 2.5 {
        // erf(x) = 2/sqrt(pi) * e^(-x^2) * sum_n 2^n x^(2n+1) / (1*3*...*(2n+1))
        let mut term = x;
        let mut sum = x;
        let mut n = 0.0;
        while term > 1e-17 * sum {
            n += 1.0;
            term *= 2.0 * x * x / (2.0 * n + 1.0);
            sum += term;
        }
        return 1.0 - 2.0 / PI.sqrt() * (-x * x).exp() * sum;
    }
    // erfc(x) = e^(-x^2)/sqrt(pi) / (x + (1/2)/(x + 1/(x + (3/2)/(x + ...))))
    let mut f = x;
    for k in (1..=80).rev() {
        f = x + (k as f64 / 2.0) / f;
    }
    (-x * x).exp() / PI.sqrt() / f
}

/// Standard normal cumulative distribution function.
pub fn norm_cdf(x: f64) -> f64 {
    0.5 * erfc(-x / SQRT_2)
}

/// Standard normal density.
pub fn norm_pdf(x: f64) -> f64 {
    (-0.5 * x * x).exp() / (2.0 * PI).sqrt()
}

fn d1_d2(forward: f64, strike: f64, t: f64, vol: f64) -> (f64, f64) {
    let sd = vol * t.sqrt();
    let d1 = ((forward / strike).ln() + 0.5 * sd * sd) / sd;
    (d1, d1 - sd)
}

/// Option premium in USD.
pub fn price(kind: Kind, forward: f64, strike: f64, t: f64, vol: f64) -> f64 {
    let (d1, d2) = d1_d2(forward, strike, t, vol);
    match kind {
        Kind::Call => forward * norm_cdf(d1) - strike * norm_cdf(d2),
        Kind::Put => strike * norm_cdf(-d2) - forward * norm_cdf(-d1),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Greeks {
    /// dPrice/dForward (unitless; 0.25 = behaves like 0.25 BTC)
    pub delta: f64,
    /// dDelta/dForward, per USD
    pub gamma: f64,
    /// USD change for a 1 volatility point (0.01) rise in implied vol
    pub vega: f64,
    /// USD change per calendar day, all else equal (time decay)
    pub theta: f64,
}

pub fn greeks(kind: Kind, forward: f64, strike: f64, t: f64, vol: f64) -> Greeks {
    let (d1, _) = d1_d2(forward, strike, t, vol);
    let sqrt_t = t.sqrt();
    let pdf = norm_pdf(d1);
    Greeks {
        delta: match kind {
            Kind::Call => norm_cdf(d1),
            Kind::Put => norm_cdf(d1) - 1.0,
        },
        gamma: pdf / (forward * vol * sqrt_t),
        vega: forward * pdf * sqrt_t / 100.0,
        theta: -forward * pdf * vol / (2.0 * sqrt_t) / 365.0,
    }
}

/// Volatility that makes the model price equal `target` (USD).
///
/// Newton's method on vega, kept inside a bisection bracket so it can never
/// diverge. Returns `None` when no volatility can produce the price (below
/// intrinsic value or above the no-arbitrage upper bound).
pub fn implied_vol(kind: Kind, target: f64, forward: f64, strike: f64, t: f64) -> Option<f64> {
    if !(target.is_finite() && forward > 0.0 && strike > 0.0 && t > 0.0) {
        return None;
    }
    let (intrinsic, upper) = match kind {
        Kind::Call => ((forward - strike).max(0.0), forward),
        Kind::Put => ((strike - forward).max(0.0), strike),
    };
    if target <= intrinsic || target >= upper {
        return None;
    }

    let (mut lo, mut hi) = (1e-6, 20.0);
    // Brenner-Subrahmanyam ATM approximation as a starting point.
    let mut vol = ((2.0 * PI / t).sqrt() * target / forward).clamp(0.05, 3.0);
    for _ in 0..100 {
        let diff = price(kind, forward, strike, t, vol) - target;
        if diff.abs() < 1e-9 {
            return Some(vol);
        }
        // Price rises with vol, so the sign tells us which side the answer is on.
        if diff > 0.0 {
            hi = vol;
        } else {
            lo = vol;
        }
        let vega = greeks(kind, forward, strike, t, vol).vega * 100.0;
        let newton = vol - diff / vega;
        let next = if vega > 1e-12 && newton > lo && newton < hi {
            newton
        } else {
            0.5 * (lo + hi)
        };
        if (next - vol).abs() < 1e-14 {
            return Some(next);
        }
        vol = next;
    }
    Some(vol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_cdf_reference_values() {
        assert!((norm_cdf(0.0) - 0.5).abs() < 1e-15);
        assert!((norm_cdf(1.96) - 0.9750021048517796).abs() < 1e-13);
        assert!((norm_cdf(-3.0) - 0.001349898031630096).abs() < 1e-15);
        assert!(norm_cdf(-40.0) >= 0.0 && norm_cdf(40.0) <= 1.0);
    }

    #[test]
    fn atm_call_reference_value() {
        // F = K = 100, T = 1, vol = 20%: C = 100 * (2 N(0.1) - 1)
        let c = price(Kind::Call, 100.0, 100.0, 1.0, 0.2);
        assert!((c - 7.965567455405798).abs() < 1e-10, "{c}");
    }

    #[test]
    fn put_call_parity() {
        for &(f, k, t, v) in &[(65_000.0, 50_000.0, 0.1, 0.5), (65_000.0, 90_000.0, 1.2, 0.8),
                               (100.0, 100.0, 0.01, 1.5)] {
            let lhs = price(Kind::Call, f, k, t, v) - price(Kind::Put, f, k, t, v);
            assert!((lhs - (f - k)).abs() < 1e-7 * f, "{lhs} vs {}", f - k);
        }
    }

    #[test]
    fn implied_vol_round_trip() {
        let f = 65_000.0;
        for &k in &[40_000.0, 60_000.0, 65_000.0, 70_000.0, 120_000.0] {
            for &t in &[2.0 / 365.0, 0.25, 1.5] {
                for &v in &[0.2, 0.5, 1.2] {
                    for kind in [Kind::Call, Kind::Put] {
                        let p = price(kind, f, k, t, v);
                        let intrinsic = match kind {
                            Kind::Call => (f - k).max(0.0),
                            Kind::Put => (k - f).max(0.0),
                        };
                        if p - intrinsic < 1.0 {
                            continue; // under $1 of time value: vol is not identifiable
                        }
                        let iv = implied_vol(kind, p, f, k, t).unwrap();
                        assert!((iv - v).abs() < 1e-6, "k={k} t={t} v={v} got {iv}");
                    }
                }
            }
        }
    }

    #[test]
    fn implied_vol_rejects_impossible_prices() {
        assert!(implied_vol(Kind::Call, 4_000.0, 65_000.0, 60_000.0, 0.5).is_none()); // < intrinsic
        assert!(implied_vol(Kind::Call, 70_000.0, 65_000.0, 60_000.0, 0.5).is_none()); // > forward
        assert!(implied_vol(Kind::Put, 1.0, 65_000.0, 60_000.0, 0.0).is_none()); // expired
    }

    #[test]
    fn greeks_match_finite_differences() {
        let (f, k, t, v) = (65_000.0, 70_000.0, 0.3, 0.55);
        for kind in [Kind::Call, Kind::Put] {
            let g = greeks(kind, f, k, t, v);
            let p = |f: f64, t: f64, v: f64| price(kind, f, k, t, v);
            let delta = (p(f + 1.0, t, v) - p(f - 1.0, t, v)) / 2.0;
            let h = 50.0; // wider step for the second derivative: less rounding noise
            let gamma = (p(f + h, t, v) - 2.0 * p(f, t, v) + p(f - h, t, v)) / (h * h);
            let vega = (p(f, t, v + 1e-5) - p(f, t, v - 1e-5)) / 2e-5 / 100.0;
            let day = 1.0 / 365.0;
            let theta = p(f, t - day, v) - p(f, t, v);
            assert!((g.delta - delta).abs() < 1e-6);
            assert!(((g.gamma - gamma) / g.gamma).abs() < 1e-4);
            assert!((g.vega - vega).abs() < 1e-4);
            assert!((g.theta - theta).abs() / theta.abs() < 0.01);
        }
    }
}
