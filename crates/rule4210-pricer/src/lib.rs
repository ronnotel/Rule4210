// rule4210-pricer — pricing engines
//
// Public surface:
//   Pricer trait          — generic repricing interface used by the scenario engine
//   MarketContext         — spot, vol, rate, div_yield at a point in time
//   BlackScholesPricer    — analytic BS for European options; used for scenario repricing
//   OptionChain / loader  — typed representation of a JSON option chain (Tradier-compatible)

use rule4210_core::{EquityOption, Instrument, OptionType};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Market context
// ---------------------------------------------------------------------------

/// The market inputs needed to price a single instrument.
/// In production this would be enriched per-instrument from a vol surface;
/// for the demo we use a flat vol with a simple skew adjustment.
#[derive(Debug, Clone)]
pub struct MarketContext {
    pub spot:      f64,   // underlying price
    pub vol:       f64,   // annualised implied vol (flat / ATM)
    pub rate:      f64,   // continuously-compounded risk-free rate
    pub div_yield: f64,   // continuously-compounded dividend yield
}

// ---------------------------------------------------------------------------
// Pricer trait
// ---------------------------------------------------------------------------

pub trait Pricer {
    /// Price `instrument` under `ctx`.  Returns None only for degenerate inputs.
    fn price(&self, instrument: &Instrument, ctx: &MarketContext) -> Option<f64>;
}

// ---------------------------------------------------------------------------
// Black-Scholes pricer
// ---------------------------------------------------------------------------

/// Prices European options via Black-Scholes-Merton (with continuous dividend yield).
/// Used for scenario repricing — the vol in MarketContext is used directly.
pub struct BlackScholesPricer;

impl Pricer for BlackScholesPricer {
    fn price(&self, instrument: &Instrument, ctx: &MarketContext) -> Option<f64> {
        match instrument {
            Instrument::Stock { .. } => Some(ctx.spot),
            Instrument::Option(opt)  => bs_price(ctx, opt),
        }
    }
}

fn bs_price(ctx: &MarketContext, opt: &EquityOption) -> Option<f64> {
    let s = ctx.spot;
    let k = opt.strike;
    let r = ctx.rate;
    let q = ctx.div_yield;
    let v = ctx.vol;
    let t = opt.time_to_expiry;

    if t <= 0.0 || v <= 0.0 || s <= 0.0 { return None; }

    let d1 = ((s / k).ln() + (r - q + 0.5 * v * v) * t) / (v * t.sqrt());
    let d2 = d1 - v * t.sqrt();
    let disc     = (-r * t).exp();
    let fwd_disc = (-q * t).exp();

    let price = match opt.option_type {
        OptionType::Call => s * fwd_disc * norm_cdf(d1) - k * disc * norm_cdf(d2),
        OptionType::Put  => k * disc * norm_cdf(-d2)    - s * fwd_disc * norm_cdf(-d1),
    };
    Some(price.max(0.0))
}

/// Abramowitz & Stegun rational approximation — max error < 7.5e-8.
pub fn norm_cdf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.2316419 * x.abs());
    let poly = t * (0.319381530
        + t * (-0.356563782
        + t * ( 1.781477937
        + t * (-1.821255978
        + t *   1.330274429))));
    let pdf  = (-x * x / 2.0).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let upper = 1.0 - pdf * poly;
    if x >= 0.0 { upper } else { 1.0 - upper }
}

// ---------------------------------------------------------------------------
// Option chain — JSON-loadable market data
// ---------------------------------------------------------------------------

/// A single option quote as returned by Tradier (or our static JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionQuote {
    pub strike:      f64,
    pub option_type: OptionType,
    pub bid:         f64,
    pub ask:         f64,
    pub mid:         f64,
    pub iv:          f64,
    pub delta:       f64,
    pub volume:      u64,
    pub open_interest: u64,
}

/// All options for one expiry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpiryChain {
    pub expiry:          String,
    pub dte:             u32,
    pub time_to_expiry:  f64,
    pub options:         Vec<OptionQuote>,
}

/// Full option chain for one underlying.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionChain {
    pub underlying: String,
    pub spot:       f64,
    pub rate:       f64,
    pub div_yield:  f64,
    pub timestamp:  String,
    pub expirations: Vec<ExpiryChain>,
}

impl OptionChain {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Look up the mid price for a specific option.
    pub fn mid_price(&self, expiry: &str, strike: f64, opt_type: &OptionType) -> Option<f64> {
        let chain = self.expirations.iter().find(|e| e.expiry == expiry)?;
        let quote = chain.options.iter().find(|q| {
            (q.strike - strike).abs() < 0.01 && &q.option_type == opt_type
        })?;
        Some(quote.mid)
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rule4210_core::{EquityOption, ExerciseStyle};

    fn atm_call_ctx() -> (MarketContext, EquityOption) {
        let ctx = MarketContext { spot: 100.0, vol: 0.20, rate: 0.05, div_yield: 0.0 };
        let opt = EquityOption {
            underlying: "X".into(), strike: 100.0, expiry: "2027-03-18".into(),
            time_to_expiry: 1.0, option_type: OptionType::Call,
            exercise_style: ExerciseStyle::European, multiplier: 100.0,
        };
        (ctx, opt)
    }

    #[test]
    fn bs_call_positive() {
        let (ctx, opt) = atm_call_ctx();
        let p = bs_price(&ctx, &opt).unwrap();
        assert!(p > 0.0, "price={p}");
    }

    #[test]
    fn bs_put_call_parity() {
        // C - P = S*e^(-qT) - K*e^(-rT)
        let (ctx, call_opt) = atm_call_ctx();
        let put_opt = EquityOption { option_type: OptionType::Put, ..call_opt.clone() };
        let c = bs_price(&ctx, &call_opt).unwrap();
        let p = bs_price(&ctx, &put_opt).unwrap();
        let pcp = ctx.spot * (-ctx.div_yield * call_opt.time_to_expiry).exp()
                - call_opt.strike * (-ctx.rate * call_opt.time_to_expiry).exp();
        assert!((c - p - pcp).abs() < 1e-6, "PCP error: {}", c - p - pcp);
    }

    #[test]
    fn bs_zero_time_returns_none() {
        let ctx = MarketContext { spot: 100.0, vol: 0.20, rate: 0.05, div_yield: 0.0 };
        let opt = EquityOption {
            underlying: "X".into(), strike: 100.0, expiry: "2026-03-18".into(),
            time_to_expiry: 0.0, option_type: OptionType::Call,
            exercise_style: ExerciseStyle::European, multiplier: 100.0,
        };
        assert!(bs_price(&ctx, &opt).is_none());
    }

    #[test]
    fn bs_stock_price_is_spot() {
        let ctx = MarketContext { spot: 560.0, vol: 0.20, rate: 0.05, div_yield: 0.013 };
        let pricer = BlackScholesPricer;
        let instr = Instrument::Stock { symbol: "SPY".into() };
        assert_eq!(pricer.price(&instr, &ctx), Some(560.0));
    }

    #[test]
    fn bs_atm_call_approaches_known_value() {
        // ATM call: S=100, K=100, r=5%, σ=20%, T=1yr, q=0 → BS ≈ $10.45
        let (ctx, opt) = atm_call_ctx();
        let p = bs_price(&ctx, &opt).unwrap();
        assert!((p - 10.45).abs() < 0.05, "expected ~10.45, got {p:.4}");
    }

    #[test]
    fn norm_cdf_symmetry() {
        for x in [0.5_f64, 1.0, 1.96, 2.576] {
            assert!((norm_cdf(x) + norm_cdf(-x) - 1.0).abs() < 1e-7, "x={x}");
        }
    }

    #[test]
    fn norm_cdf_half_at_zero() {
        assert!((norm_cdf(0.0) - 0.5).abs() < 1e-7);
    }
}
