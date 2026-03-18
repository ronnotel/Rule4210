// rule4210-scenarios — TIMS scenario engine
//
// Implements the scenario set mandated by FINRA Rule 4210(g) for portfolio
// margin accounts:
//
//   • 10 equidistant price points across ±15% of current underlying price
//     (equity / ETF options; ±10% narrow-based index, ±6% broad-based index)
//   • 2 extreme scenarios at ±150% of the normal range (±22.5%) with a
//     ±4 vol-point shock — these can dominate for large short-option positions
//
// The ScenarioEngine reprices every position under each scenario using the
// supplied Pricer (BlackScholesPricer for this demo; CRR for production).
// Position P&L = (new_price − base_price) × quantity × multiplier.

use rule4210_core::{Instrument, Position};
use rule4210_pricer::{MarketContext, Pricer};

// ---------------------------------------------------------------------------
// Scenario definition
// ---------------------------------------------------------------------------

/// One stress scenario: a simultaneous shock to spot and implied vol.
#[derive(Debug, Clone)]
pub struct Scenario {
    /// Fractional spot move: 0.10 = +10%, -0.15 = −15%.
    pub spot_shock: f64,
    /// Absolute vol change (annualised): 0.04 = +4 vol points.
    pub vol_shock: f64,
    pub label: String,
}

/// The TIMS / Rule 4210(g) standard scenario set for equity options.
///
/// 10 equidistant price points over [−15%, +15%] with no vol shock,
/// plus 2 extreme scenarios at ±22.5% with a ±4 vol-point shock.
pub fn tims_scenarios() -> Vec<Scenario> {
    let mut scenarios = Vec::with_capacity(12);

    // 10 price points: −15% to +15% in steps of 30/9 ≈ 3.33 pp
    for i in 0..10usize {
        let shock = -0.15 + i as f64 * (0.30 / 9.0);
        scenarios.push(Scenario {
            spot_shock: shock,
            vol_shock:  0.0,
            label: format!("spot {:+.1}%", shock * 100.0),
        });
    }

    // Extreme down: −22.5% spot, vol +4 pts  (Rule 4210 "double-the-range" check)
    scenarios.push(Scenario {
        spot_shock: -0.225,
        vol_shock:   0.04,
        label: "extreme −22.5%  vol +4pt".to_string(),
    });

    // Extreme up: +22.5% spot, vol −4 pts
    scenarios.push(Scenario {
        spot_shock:  0.225,
        vol_shock:  -0.04,
        label: "extreme +22.5%  vol −4pt".to_string(),
    });

    scenarios
}

// ---------------------------------------------------------------------------
// Per-position and aggregate P&L
// ---------------------------------------------------------------------------

/// P&L for a single position under one scenario.
#[derive(Debug, Clone)]
pub struct PositionPnl {
    pub label:    String,
    pub quantity: f64,
    pub pnl:      f64,
}

/// Aggregate result across all positions for one scenario.
#[derive(Debug, Clone)]
pub struct ScenarioResult {
    pub scenario:      Scenario,
    pub position_pnls: Vec<PositionPnl>,
    pub total_pnl:     f64,
}

// ---------------------------------------------------------------------------
// Scenario engine
// ---------------------------------------------------------------------------

pub struct ScenarioEngine<'a> {
    pricer: &'a dyn Pricer,
}

impl<'a> ScenarioEngine<'a> {
    pub fn new(pricer: &'a dyn Pricer) -> Self {
        Self { pricer }
    }

    /// Run all `scenarios` over `positions` using `base_ctx` as the t=0 market.
    pub fn run(
        &self,
        positions:  &[&Position],
        base_ctx:   &MarketContext,
        scenarios:  &[Scenario],
    ) -> Vec<ScenarioResult> {
        // Base prices — computed once so that all scenario deltas are consistent.
        let base_prices: Vec<f64> = positions
            .iter()
            .map(|p| self.pricer.price(&p.instrument, base_ctx).unwrap_or(0.0))
            .collect();

        scenarios
            .iter()
            .map(|scenario| {
                let shocked_ctx = MarketContext {
                    spot: base_ctx.spot * (1.0 + scenario.spot_shock),
                    vol:  (base_ctx.vol + scenario.vol_shock).max(0.001),
                    ..*base_ctx
                };

                let position_pnls: Vec<PositionPnl> = positions
                    .iter()
                    .zip(base_prices.iter())
                    .map(|(pos, &base_price)| {
                        let new_price = self
                            .pricer
                            .price(&pos.instrument, &shocked_ctx)
                            .unwrap_or(0.0);
                        let multiplier = multiplier_of(&pos.instrument);
                        PositionPnl {
                            label:    pos.instrument.label(),
                            quantity: pos.quantity,
                            pnl:      pos.quantity * (new_price - base_price) * multiplier,
                        }
                    })
                    .collect();

                let total_pnl = position_pnls.iter().map(|pp| pp.pnl).sum();

                ScenarioResult {
                    scenario: scenario.clone(),
                    position_pnls,
                    total_pnl,
                }
            })
            .collect()
    }
}

fn multiplier_of(instrument: &Instrument) -> f64 {
    match instrument {
        Instrument::Option(o) => o.multiplier,
        Instrument::Stock { .. } => 1.0,
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rule4210_core::{EquityOption, ExerciseStyle, OptionType};
    use rule4210_pricer::BlackScholesPricer;

    #[test]
    fn tims_has_twelve_scenarios() {
        assert_eq!(tims_scenarios().len(), 12);
    }

    #[test]
    fn tims_price_range_is_correct() {
        let s = tims_scenarios();
        let standard: Vec<&Scenario> = s.iter().filter(|sc| sc.vol_shock == 0.0).collect();
        let extreme:  Vec<&Scenario> = s.iter().filter(|sc| sc.vol_shock != 0.0).collect();
        assert_eq!(standard.len(), 10);
        assert_eq!(extreme.len(), 2);
        // First standard scenario is −15%
        assert!((standard[0].spot_shock - (-0.15)).abs() < 1e-9);
        // Last standard scenario is +15%
        assert!((standard[9].spot_shock - 0.15).abs() < 1e-9);
    }

    #[test]
    fn tims_extreme_scenarios_correct() {
        let s = tims_scenarios();
        let extremes: Vec<&Scenario> = s.iter().filter(|sc| sc.vol_shock != 0.0).collect();
        assert!((extremes[0].spot_shock - (-0.225)).abs() < 1e-9);
        assert!((extremes[1].spot_shock -  0.225).abs()  < 1e-9);
        assert!((extremes[0].vol_shock  -  0.04).abs()   < 1e-9);
        assert!((extremes[1].vol_shock  - (-0.04)).abs() < 1e-9);
    }

    fn long_call_position() -> Position {
        Position::new(
            Instrument::Option(EquityOption {
                underlying: "X".into(), strike: 100.0, expiry: "2027-03-18".into(),
                time_to_expiry: 1.0, option_type: OptionType::Call,
                exercise_style: ExerciseStyle::European, multiplier: 100.0,
            }),
            1.0,  // long 1 contract
            10.0,
        )
    }

    #[test]
    fn long_call_gains_on_up_scenario() {
        let pricer = BlackScholesPricer;
        let engine = ScenarioEngine::new(&pricer);
        let pos = long_call_position();
        let ctx = MarketContext { spot: 100.0, vol: 0.20, rate: 0.05, div_yield: 0.0 };
        let up_scenario = vec![Scenario { spot_shock: 0.10, vol_shock: 0.0, label: "up10".into() }];
        let results = engine.run(&[&pos], &ctx, &up_scenario);
        assert!(results[0].total_pnl > 0.0, "long call should gain on up move");
    }

    #[test]
    fn long_call_loses_on_down_scenario() {
        let pricer = BlackScholesPricer;
        let engine = ScenarioEngine::new(&pricer);
        let pos = long_call_position();
        let ctx = MarketContext { spot: 100.0, vol: 0.20, rate: 0.05, div_yield: 0.0 };
        let dn_scenario = vec![Scenario { spot_shock: -0.10, vol_shock: 0.0, label: "dn10".into() }];
        let results = engine.run(&[&pos], &ctx, &dn_scenario);
        assert!(results[0].total_pnl < 0.0, "long call should lose on down move");
    }

    #[test]
    fn flat_scenario_zero_pnl() {
        let pricer = BlackScholesPricer;
        let engine = ScenarioEngine::new(&pricer);
        let pos = long_call_position();
        let ctx = MarketContext { spot: 100.0, vol: 0.20, rate: 0.05, div_yield: 0.0 };
        let flat = vec![Scenario { spot_shock: 0.0, vol_shock: 0.0, label: "flat".into() }];
        let results = engine.run(&[&pos], &ctx, &flat);
        assert!(results[0].total_pnl.abs() < 1e-6, "flat scenario should have zero P&L");
    }

    #[test]
    fn long_stock_pnl_proportional_to_shock() {
        let pricer = BlackScholesPricer;
        let engine = ScenarioEngine::new(&pricer);
        let pos = Position::new(
            Instrument::Stock { symbol: "X".into() },
            100.0, 560.0,
        );
        let ctx = MarketContext { spot: 560.0, vol: 0.20, rate: 0.05, div_yield: 0.0 };
        let scenario = vec![Scenario { spot_shock: 0.10, vol_shock: 0.0, label: "up10".into() }];
        let results = engine.run(&[&pos], &ctx, &scenario);
        let expected = 100.0 * 560.0 * 0.10;  // 100 shares × $560 × 10%
        assert!((results[0].total_pnl - expected).abs() < 0.01,
            "got {:.2}, expected {expected:.2}", results[0].total_pnl);
    }
}
