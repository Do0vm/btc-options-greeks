//! btc-options-greeks <chain.json> [--expiry 27DEC26] [--csv out.csv]
//!
//! Get a live chain with:
//!   curl -s "https://www.deribit.com/api/v2/public/get_book_summary_by_currency?currency=BTC&kind=option" > chain.json

use std::fs;
use std::io::Write;
use std::process::ExitCode;

use btc_options_greeks::analysis::{agreement, analyse, smiles, Row};
use btc_options_greeks::black76::Kind;
use btc_options_greeks::deribit::Response;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: btc-options-greeks <chain.json> [--expiry 27DEC26] [--csv out.csv]");
        return ExitCode::from(2);
    };
    let flag = |name: &str| {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };

    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let chain: Response = match serde_json::from_str(&text) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("not a Deribit book summary response: {e}");
            return ExitCode::FAILURE;
        }
    };

    let (rows, skipped) = analyse(&chain.result);
    println!("Options in file: {}   priced: {}   skipped: {}", chain.result.len(), rows.len(), skipped);

    if let Some(a) = agreement(rows.iter().filter(|r| r.otm)) {
        println!(
            "IV vs Deribit mark_iv, out-of-the-money options (n={}): median {:.3}, p95 {:.3}, max {:.3} vol points",
            a.n, a.median, a.p95, a.max
        );
    }
    if let Some(a) = agreement(rows.iter().filter(|r| !r.otm)) {
        println!(
            "IV vs Deribit mark_iv, in-the-money options (n={}):     median {:.3}, p95 {:.3}, max {:.3} vol points",
            a.n, a.median, a.p95, a.max
        );
    }

    println!("\nVolatility term structure and skew (out-of-the-money options)");
    println!("{:>9} {:>7} {:>10} {:>8} {:>9} {:>9} {:>5}", "expiry", "days", "forward", "ATM IV", "25d RR", "25d BF", "n");
    for s in smiles(&rows) {
        let pts = |x: Option<f64>| x.map_or("-".to_string(), |v| format!("{:+.2}", v * 100.0));
        println!(
            "{:>9} {:>7.2} {:>10.0} {:>7.2}% {:>9} {:>9} {:>5}",
            s.expiry, s.days, s.forward, s.atm_iv * 100.0, pts(s.risk_reversal_25d), pts(s.butterfly_25d), s.options
        );
    }

    if let Some(exp) = flag("--expiry") {
        print_smile(&rows, &exp);
    }
    if let Some(out) = flag("--csv") {
        if let Err(e) = write_csv(&rows, &out) {
            eprintln!("cannot write {out}: {e}");
            return ExitCode::FAILURE;
        }
        println!("\nwrote {} rows to {out}", rows.len());
    }
    ExitCode::SUCCESS
}

fn print_smile(rows: &[Row], expiry: &str) {
    println!("\nSmile for {expiry} (out-of-the-money options)");
    println!(
        "{:>8} {:>4} {:>8} {:>8} {:>8} {:>7} {:>7} {:>10} {:>9} {:>9}",
        "strike", "type", "ln(K/F)", "our IV", "Deribit", "diff", "delta", "gamma", "vega $", "theta $/d"
    );
    for r in rows.iter().filter(|r| r.otm && r.expiry == expiry) {
        println!(
            "{:>8.0} {:>4} {:>8.3} {:>7.2}% {:>7.2}% {:>7.3} {:>7.3} {:>10.2e} {:>9.2} {:>9.2}",
            r.strike,
            if r.kind == Kind::Call { "C" } else { "P" },
            r.log_moneyness(),
            r.iv * 100.0,
            r.deribit_iv * 100.0,
            r.iv_diff_points(),
            r.greeks.delta,
            r.greeks.gamma,
            r.greeks.vega,
            r.greeks.theta
        );
    }
}

fn write_csv(rows: &[Row], path: &str) -> std::io::Result<()> {
    let mut f = std::io::BufWriter::new(fs::File::create(path)?);
    writeln!(f, "instrument,expiry,days,strike,type,forward,premium_usd,iv,deribit_iv,otm,delta,gamma,vega,theta")?;
    for r in rows {
        writeln!(
            f,
            "{},{},{:.4},{},{},{:.2},{:.4},{:.6},{:.6},{},{:.6},{:.4e},{:.4},{:.4}",
            r.name, r.expiry, r.days, r.strike,
            if r.kind == Kind::Call { "C" } else { "P" },
            r.forward, r.premium_usd, r.iv, r.deribit_iv, r.otm,
            r.greeks.delta, r.greeks.gamma, r.greeks.vega, r.greeks.theta
        )?;
    }
    Ok(())
}
