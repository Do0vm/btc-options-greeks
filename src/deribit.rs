//! Parsing Deribit's public `get_book_summary_by_currency` response.

use serde::Deserialize;

use crate::black76::Kind;

#[derive(Debug, Deserialize)]
pub struct Response {
    pub result: Vec<BookSummary>,
}

/// One row of the response. Fields Deribit leaves null are `None`.
#[derive(Debug, Clone, Deserialize)]
pub struct BookSummary {
    pub instrument_name: String,
    pub mark_price: Option<f64>, // option premium, in BTC
    pub mark_iv: Option<f64>,    // Deribit's own implied vol, in percent
    pub underlying_price: Option<f64>, // forward price for this expiry, USD
    pub creation_timestamp: i64, // ms since epoch
    pub bid_price: Option<f64>,
    pub ask_price: Option<f64>,
    pub open_interest: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instrument {
    pub underlying: String,
    pub expiry_code: String, // e.g. "27DEC26"
    pub expiry_unix: i64,    // seconds; Deribit options expire at 08:00 UTC
    pub strike: f64,
    pub kind: Kind,
}

/// Parse names like `BTC-27DEC26-100000-C`. Returns `None` for anything else
/// (futures, perpetuals, malformed names).
pub fn parse_instrument(name: &str) -> Option<Instrument> {
    let mut parts = name.split('-');
    let underlying = parts.next()?.to_string();
    let expiry_code = parts.next()?.to_string();
    let strike: f64 = parts.next()?.replace('d', ".").parse().ok()?;
    let kind = match parts.next()? {
        "C" => Kind::Call,
        "P" => Kind::Put,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    let expiry_unix = expiry_to_unix(&expiry_code)?;
    Some(Instrument { underlying, expiry_code, expiry_unix, strike, kind })
}

/// "27DEC26" -> unix seconds at 08:00 UTC on 2026-12-27.
pub fn expiry_to_unix(code: &str) -> Option<i64> {
    if code.len() < 6 || !code.is_ascii() {
        return None;
    }
    let (day, rest) = code.split_at(code.len() - 5);
    let (mon, year) = rest.split_at(3);
    let day: u32 = day.parse().ok()?;
    let year: i64 = 2000 + year.parse::<i64>().ok()?;
    let month = match mon {
        "JAN" => 1, "FEB" => 2, "MAR" => 3, "APR" => 4, "MAY" => 5, "JUN" => 6,
        "JUL" => 7, "AUG" => 8, "SEP" => 9, "OCT" => 10, "NOV" => 11, "DEC" => 12,
        _ => return None,
    };
    if !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + 8 * 3_600)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
        assert_eq!(days_from_civil(2026, 11, 20), 20_777);
    }

    #[test]
    fn parses_option_names() {
        let i = parse_instrument("BTC-20NOV26-64000-P").unwrap();
        assert_eq!(i.underlying, "BTC");
        assert_eq!(i.strike, 64_000.0);
        assert_eq!(i.kind, Kind::Put);
        assert_eq!(i.expiry_unix, 20_777 * 86_400 + 8 * 3_600);
        assert!(parse_instrument("BTC-5JAN27-70000-C").is_some()); // single-digit day
        assert!(parse_instrument("BTC-PERPETUAL").is_none());
        assert!(parse_instrument("BTC-27DEC26").is_none());
        assert!(parse_instrument("BTC-27XYZ26-1000-C").is_none());
    }
}
