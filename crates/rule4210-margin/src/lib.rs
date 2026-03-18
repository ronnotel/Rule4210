// rule4210-margin — margin requirement calculators
//
// Portfolio Margin (TIMS)
// -----------------------
// Under FINRA Rule 4210(g), the margin requirement for a portfolio margin
// account is the LARGER of:
//   (a) the worst-case theoretical loss across the TIMS scenario set, OR
//   (b) the minimum margin floor: $37.50 per listed option contract
//
// The worst-case loss is always non-negative (losses are positive amounts).
//
// Regulation T (simplified)
// -------------------------
// Reg-T margin for equities and options per Regulation T and FINRA Rule 4210(c):
//   • Long stock / ETF:       50% of market value (initial)
//   • Short naked call/put:   20% of underlying value + premium - OTM amount
//   • Long options:           100% of premium (no margin relief)
//   • Covered call:           no additional margin over the long stock
//   • Defined-risk spreads:   max loss (strategy margin)
//
// This implementation uses the simplified "20% rule" for short options and
// 50% for stock — sufficient to show the PM capital efficiency gain in a demo.

use rule4210_core::{Instrument, Position};
use rule4210_scenarios::ScenarioResult;

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct MarginRequirement {
    pub method:          &'static str,
    pub amount:          f64,
    /// Label of the worst scenario (PM only).
    pub worst_scenario:  Option<String>,
}

// ---------------------------------------------------------------------------
// Portfolio margin (TIMS)
// ---------------------------------------------------------------------------

/// Minimum margin per option contract under Rule 4210(g).
pub const MIN_PER_CONTRACT: f64 = 37.50;

/// Compute the TIMS portfolio-margin requirement from a full scenario run.
///
/// `n_option_contracts` — total number of long + short option contracts
/// in the margin group (used to enforce the per-contract minimum floor).
pub fn portfolio_margin(
    results:            &[ScenarioResult],
    n_option_contracts: usize,
) -> MarginRequirement {
    let worst = results
        .iter()
        .min_by(|a, b| a.total_pnl.partial_cmp(&b.total_pnl).unwrap());

    let floor = MIN_PER_CONTRACT * n_option_contracts as f64;

    let (loss, label) = match worst {
        Some(r) => ((-r.total_pnl).max(0.0), Some(r.scenario.label.clone())),
        None    => (0.0, None),
    };

    MarginRequirement {
        method:         "Portfolio Margin (TIMS / Rule 4210(g))",
        amount:         loss.max(floor),
        worst_scenario: label,
    }
}

// ---------------------------------------------------------------------------
// Regulation T (simplified)
// ---------------------------------------------------------------------------

/// Compute a simplified Regulation T margin requirement for comparison.
///
/// Uses:
///   • Stock / ETF:   50% × market value
///   • Short option:  20% × (underlying market value × 100) − OTM amount + premium
///   • Long option:   premium paid (no margin relief vs a short)
///   • Covered calls: no additional margin over the long stock (standard rule)
///
/// `spot` is the current price of the underlying.
pub fn reg_t_margin(positions: &[&Position], spot: f64) -> MarginRequirement {
    let mut total = 0.0;
    let mut has_long_stock   = false;
    let mut short_call_count = 0usize;

    // First pass: check for covered-call situation
    for p in positions.iter() {
        if let Instrument::Stock { .. } = &p.instrument {
            if p.quantity > 0.0 { has_long_stock = true; }
        }
        if let Instrument::Option(o) = &p.instrument {
            if p.quantity < 0.0 && matches!(o.option_type, rule4210_core::OptionType::Call) {
                short_call_count += 1;
            }
        }
    }

    for p in positions.iter() {
        match &p.instrument {
            Instrument::Stock { .. } => {
                // 50% of market value
                total += p.quantity.abs() * spot * 0.50;
            }
            Instrument::Option(o) => {
                let underlying_notional = o.multiplier * spot;
                if p.quantity < 0.0 {
                    // Short option
                    let is_call = matches!(o.option_type, rule4210_core::OptionType::Call);
                    // If this short call is covered by long stock, no additional margin
                    if is_call && has_long_stock && short_call_count > 0 {
                        // Covered call: margin = 0 (stock already margined above)
                    } else {
                        // Naked: 20% of underlying notional - OTM amount + premium
                        let otm_amount = match o.option_type {
                            rule4210_core::OptionType::Call =>
                                (o.strike - spot).max(0.0) * o.multiplier,
                            rule4210_core::OptionType::Put  =>
                                (spot - o.strike).max(0.0) * o.multiplier,
                        };
                        let premium = p.cost_basis * o.multiplier;
                        total += p.quantity.abs()
                            * (0.20 * underlying_notional - otm_amount + premium)
                               .max(0.10 * underlying_notional);
                    }
                }
                // Long options: no margin requirement (fully paid for)
            }
        }
    }

    MarginRequirement {
        method:          "Regulation T",
        amount:          total,
        worst_scenario:  None,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rule4210_core::{EquityOption, ExerciseStyle, Instrument, OptionType, Position};
    use rule4210_scenarios::{Scenario, ScenarioResult};

    fn make_scenario_result(pnl: f64, label: &str) -> ScenarioResult {
        ScenarioResult {
            scenario: Scenario { spot_shock: 0.0, vol_shock: 0.0, label: label.to_string() },
            position_pnls: vec![],
            total_pnl: pnl,
        }
    }

    #[test]
    fn pm_worst_case_is_largest_loss() {
        let results = vec![
            make_scenario_result( 500.0, "up"),
            make_scenario_result(-2000.0, "down"),
            make_scenario_result(-1500.0, "extreme"),
        ];
        let req = portfolio_margin(&results, 2);
        // Worst loss is $2,000; floor = 37.50 × 2 = $75 → PM = $2,000
        assert!((req.amount - 2000.0).abs() < 0.01, "got {}", req.amount);
        assert_eq!(req.worst_scenario.as_deref(), Some("down"));
    }

    #[test]
    fn pm_floor_applied_when_loss_is_small() {
        let results = vec![make_scenario_result(-10.0, "tiny loss")];
        // 4 contracts × $37.50 = $150 floor > $10 loss
        let req = portfolio_margin(&results, 4);
        assert!((req.amount - 150.0).abs() < 0.01, "got {}", req.amount);
    }

    #[test]
    fn pm_no_margin_for_all_profitable_scenarios() {
        let results = vec![
            make_scenario_result(100.0, "up"),
            make_scenario_result(200.0, "down"),
        ];
        // All scenarios profitable; floor = 37.50 × 2 = $75
        let req = portfolio_margin(&results, 2);
        assert!((req.amount - 75.0).abs() < 0.01, "got {}", req.amount);
    }

    #[test]
    fn regt_long_stock_fifty_percent() {
        let pos = Position::new(Instrument::Stock { symbol: "SPY".into() }, 100.0, 560.0);
        let req = reg_t_margin(&[&pos], 560.0);
        // 100 shares × $560 × 50% = $28,000
        assert!((req.amount - 28_000.0).abs() < 0.01, "got {}", req.amount);
    }

    #[test]
    fn regt_long_option_no_margin() {
        let pos = Position::new(
            Instrument::Option(EquityOption {
                underlying: "SPY".into(), strike: 540.0, expiry: "2026-04-17".into(),
                time_to_expiry: 30.0 / 365.0, option_type: OptionType::Put,
                exercise_style: ExerciseStyle::American, multiplier: 100.0,
            }),
            1.0,   // long put
            3.81,
        );
        let req = reg_t_margin(&[&pos], 560.0);
        assert!((req.amount - 0.0).abs() < 0.01, "long option should have zero margin, got {}", req.amount);
    }

    #[test]
    fn pm_method_label_correct() {
        let results = vec![make_scenario_result(-500.0, "s1")];
        let req = portfolio_margin(&results, 1);
        assert!(req.method.contains("Portfolio Margin"));
        assert!(req.method.contains("Rule 4210"));
    }
}
