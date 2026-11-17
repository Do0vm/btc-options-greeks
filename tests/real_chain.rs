//! Integration test on a real Deribit BTC option chain captured 2026-11-17.

use btc_options_greeks::analysis::{agreement, analyse, smiles};
use btc_options_greeks::deribit::Response;

fn load() -> Response {
    let text = std::fs::read_to_string("data/deribit_btc_2026-11-17.json").unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn prices_almost_the_whole_chain() {
    let chain = load();
    let (rows, skipped) = analyse(&chain.result);
    assert_eq!(chain.result.len(), 954);
    assert!(rows.len() > 900, "priced {} skipped {}", rows.len(), skipped);
}

#[test]
fn matches_deribit_implied_vol_on_otm_options() {
    let (rows, _) = analyse(&load().result);
    let a = agreement(rows.iter().filter(|r| r.otm)).unwrap();
    assert!(a.n > 450, "{a:?}");
    assert!(a.median < 0.05, "{a:?}");
    assert!(a.max < 0.25, "{a:?}");
}

#[test]
fn greeks_are_sane() {
    let (rows, _) = analyse(&load().result);
    for r in &rows {
        assert!(r.greeks.gamma > 0.0 && r.greeks.vega >= 0.0 && r.greeks.theta <= 0.0, "{}", r.name);
        assert!(r.greeks.delta.abs() <= 1.0, "{}", r.name);
    }
}

#[test]
fn smiles_cover_every_expiry() {
    let (rows, _) = analyse(&load().result);
    let s = smiles(&rows);
    assert!(s.len() >= 8);
    for x in &s {
        assert!(x.atm_iv > 0.1 && x.atm_iv < 2.0, "{x:?}");
    }
}
