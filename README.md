# btc-options-greeks

[![CI](https://github.com/Do0vm/btc-options-greeks/actions/workflows/ci.yml/badge.svg)](https://github.com/Do0vm/btc-options-greeks/actions/workflows/ci.yml)

**Rust library and CLI that prices Deribit BTC options**: Black-76 model, Greeks, implied-volatility solver, and per-expiry volatility smile and skew metrics.

On a real chain of 954 options it reproduces **Deribit's own mark IV to within 0.14 volatility points on every out-of-the-money option** (median 0.012).

- **Normal CDF from scratch:** Rust's `std` has no `erf`, so `erfc` uses a positive-term series near zero and a continued fraction in the tails (about 1e-12 relative error, checked against reference values)
- **Implied vol solver:** Newton's method on vega, kept inside a bisection bracket so it can't diverge; returns `None` for prices no volatility can produce
- **Greeks:** delta, gamma, vega (USD per vol point) and theta (USD per day), verified against finite differences
- **Smile analytics:** ATM vol term structure, 25-delta risk reversal and butterfly per expiry
- **Tests:** 12 unit and integration tests run on a captured chain, plus clippy with `-D warnings`. CI also downloads a **live** chain from Deribit on every push and runs the tool on it.

## Output (captured chain, 17 Nov 2026, BTC ≈ $65,000)

```
Options in file: 954   priced: 950   skipped: 4
IV vs Deribit mark_iv, out-of-the-money options (n=477): median 0.012, p95 0.066, max 0.140 vol points
IV vs Deribit mark_iv, in-the-money options (n=473):     median 0.156, p95 7.120, max 46.736 vol points

Volatility term structure and skew (out-of-the-money options)
   expiry    days    forward   ATM IV    25d RR    25d BF     n
  18NOV26    0.82      64805   47.38%     -8.86     +2.91    31
  20NOV26    2.82      64831   48.79%     -5.97     +1.49    40
  27NOV26    9.82      64899   43.92%     -5.52     +0.89    60
  25DEC26   37.82      65144   41.52%     -4.96     +1.00    60
  29JAN27   72.82      65496   40.99%     -5.22     +1.05    57
  26MAR27  128.82      66066   41.09%     -3.95     +0.68    52
  24SEP27  310.82      67977   41.22%     -2.99     +0.57    53

Smile for 27NOV26 (out-of-the-money options)
  strike type  ln(K/F)   our IV  Deribit    diff   delta      gamma    vega $ theta $/d
   55000    P   -0.165   59.41%   59.40%   0.007  -0.040    1.37e-5      9.23    -27.93
   60000    P   -0.078   50.78%   50.76%   0.019  -0.163    4.55e-5     26.18    -67.66
   64000    P   -0.014   44.99%   44.96%   0.026  -0.411    8.12e-5     41.40    -94.81
   65000    C    0.002   43.92%   43.94%  -0.022   0.506    8.53e-5     42.47    -94.94
   70000    C    0.076   42.47%   42.48%  -0.014   0.147    5.08e-5     24.44    -52.83
   80000    C    0.209   55.93%   55.93%  -0.003   0.013    5.51e-6      3.49     -9.95
```
(some expiry and strike rows omitted)

**Reading it:**
- **Downside skew everywhere.** The 25-delta risk reversal (call IV minus put IV) is negative at every expiry: traders pay more for crash protection than for upside.
- **Short-dated vol is highest.** ATM IV is 47–49% for the next few days and settles at about 41% further out, an inverted short end typical when the market expects near-term moves.
- **The smile is real.** On 27NOV26, IV is lowest near 68k and rises in both wings, to 59% for the 55k put and 56% for the 80k call.

**Why only out-of-the-money options?** Deep in-the-money options are almost all intrinsic value. Their implied vol depends on a few dollars of time value, so tiny price differences produce huge IV differences (up to 47 points above). Vol desks build smiles from OTM options for the same reason.

## Conventions (checked against Deribit's numbers)

- Deribit quotes BTC options **in BTC**. USD premium = `mark_price × underlying_price`, where `underlying_price` is the forward for that expiry.
- Black-76 on that forward with zero rates, matching Deribit's `interest_rate: 0`.
- Expiry is 08:00 UTC on the date in the instrument name; time is measured from `creation_timestamp` in a 365-day year.
- Delta is the plain forward delta. Deribit's own delta for BTC-settled options is **premium-adjusted**, so it differs slightly.

## Usage

```bash
curl -s "https://www.deribit.com/api/v2/public/get_book_summary_by_currency?currency=BTC&kind=option" > chain.json
cargo run --release -- chain.json                      # agreement + term structure + skew
cargo run --release -- chain.json --expiry 27NOV26     # strike-by-strike smile with Greeks
cargo run --release -- chain.json --csv out.csv        # every option to CSV
cargo test --release
```

```
src/black76.rs    erfc, normal CDF/PDF, price, Greeks, implied vol (+ unit tests)
src/deribit.rs    JSON types, instrument-name and expiry parsing, calendar maths
src/analysis.rs   chain -> rows, agreement statistics, smile metrics
tests/real_chain.rs  integration tests on data/deribit_btc_2026-11-17.json
```

## Limitations and next steps

- Mark prices only; bid/ask IVs would show where the market really is
- No smile fitting yet. SVI parameterisation per expiry would allow interpolation and arbitrage checks
- Plain forward delta; adding premium-adjusted delta would match Deribit's risk numbers for inverse options
