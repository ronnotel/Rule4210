// Rule 4210 Portfolio Margin Engine — demo
//
// Demonstrates the capital efficiency of Portfolio Margin vs Regulation T
// using a hedged SPY collar position.  All prices computed via Black-Scholes;
// in production the CRR tree (from rule4210-pricer, soon to absorb RustCRR)
// will handle American exercise and discrete dividends.
//
// Portfolio:
//   Long  100 shares SPY           @ $560.00
//   Long    1 SPY Apr-17 540 Put   @   $3.81  (downside insurance)
//   Short   1 SPY Apr-17 580 Call  @   $3.91  (premium financed upside cap)
//
// Expected result: PM requirement ≈ $2,300 vs Reg-T ≈ $28,000

use rule4210_core::{EquityOption, ExerciseStyle, Instrument, OptionType, Portfolio, Position};
use rule4210_margin::{portfolio_margin, reg_t_margin};
use rule4210_pricer::{BlackScholesPricer, MarketContext};
use rule4210_scenarios::{tims_scenarios, ScenarioEngine, ScenarioResult};

fn main() {
    // -----------------------------------------------------------------------
    // 1. Build the portfolio
    // -----------------------------------------------------------------------

    let mut portfolio = Portfolio::new("SPY Collar");

    portfolio.add(Position::new(
        Instrument::Stock { symbol: "SPY".into() },
        100.0,   // long 100 shares
        560.0,   // cost basis per share
    ));

    portfolio.add(Position::new(
        Instrument::Option(EquityOption {
            underlying:     "SPY".into(),
            strike:         540.0,
            expiry:         "2026-04-17".into(),
            time_to_expiry: 30.0 / 365.0,
            option_type:    OptionType::Put,
            exercise_style: ExerciseStyle::American,
            multiplier:     100.0,
        }),
        1.0,    // long 1 contract
        3.81,   // cost basis (mid price)
    ));

    portfolio.add(Position::new(
        Instrument::Option(EquityOption {
            underlying:     "SPY".into(),
            strike:         580.0,
            expiry:         "2026-04-17".into(),
            time_to_expiry: 30.0 / 365.0,
            option_type:    OptionType::Call,
            exercise_style: ExerciseStyle::American,
            multiplier:     100.0,
        }),
        -1.0,   // short 1 contract
        3.91,   // premium received (cost basis)
    ));

    // -----------------------------------------------------------------------
    // 2. Market context
    // -----------------------------------------------------------------------

    let ctx = MarketContext {
        spot:      560.0,
        vol:       0.172,  // ATM 30-day implied vol for SPY
        rate:      0.045,  // 4.5% risk-free (approx Mar-2026)
        div_yield: 0.013,  // ~1.3% SPY dividend yield
    };

    // -----------------------------------------------------------------------
    // 3. Run TIMS scenarios
    // -----------------------------------------------------------------------

    let pricer  = BlackScholesPricer;
    let engine  = ScenarioEngine::new(&pricer);
    let groups  = portfolio.margin_groups();
    let spy_pos = groups["SPY"].clone();
    let scenarios = tims_scenarios();

    let results = engine.run(&spy_pos, &ctx, &scenarios);

    // -----------------------------------------------------------------------
    // 4. Display scenario matrix
    // -----------------------------------------------------------------------

    print_header(&portfolio, &ctx);
    print_scenario_table(&results);

    // -----------------------------------------------------------------------
    // 5. Margin comparison
    // -----------------------------------------------------------------------

    let n_contracts = spy_pos.iter().filter(|p| matches!(p.instrument, Instrument::Option(_))).count();
    let pm  = portfolio_margin(&results, n_contracts);
    let regt = reg_t_margin(&spy_pos, ctx.spot);

    print_margin_summary(&pm, &regt);
}

// ---------------------------------------------------------------------------
// Display helpers
// ---------------------------------------------------------------------------

fn print_header(portfolio: &Portfolio, ctx: &MarketContext) {
    println!();
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║           Rule 4210 Portfolio Margin Engine — Demo              ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Portfolio: {}", portfolio.name);
    for p in &portfolio.positions {
        println!(
            "  {:>6}  {:>4.0}  {:<38}  @ {:>7.2}",
            if p.quantity > 0.0 { "Long" } else { "Short" },
            p.quantity.abs(),
            p.instrument.label(),
            p.cost_basis,
        );
    }
    println!();
    println!("Market Context (2026-03-18):");
    println!("  SPY Spot:    ${:.2}", ctx.spot);
    println!("  ATM 30d Vol:  {:.1}%", ctx.vol * 100.0);
    println!("  Risk-free:    {:.2}%", ctx.rate * 100.0);
    println!("  Div yield:    {:.2}%", ctx.div_yield * 100.0);
    println!();
}

fn print_scenario_table(results: &[ScenarioResult]) {
    println!("TIMS Scenario Analysis  (FINRA Rule 4210(g) — 12 scenarios)");
    println!();

    // Dynamic column widths from position labels
    let n_pos = results.first().map_or(0, |r| r.position_pnls.len());
    let labels: Vec<&str> = results
        .first()
        .map_or(vec![], |r| r.position_pnls.iter().map(|pp| pp.label.as_str()).collect());

    // Header row
    print!("  {:<30}", "Scenario");
    for lbl in &labels {
        let short = truncate(lbl, 14);
        print!("  {:>14}", short);
    }
    println!("  {:>12}  ", "Total P&L");

    // Separator
    print!("  {}", "─".repeat(30));
    for _ in 0..n_pos { print!("  {}", "─".repeat(14)); }
    println!("  {}  ", "─".repeat(12));

    // Find worst-case row for highlighting
    let worst_total = results.iter().map(|r| r.total_pnl).fold(f64::INFINITY, f64::min);

    for r in results {
        let is_worst = (r.total_pnl - worst_total).abs() < 0.01;
        let marker = if is_worst { " ◄ WORST" } else { "" };
        print!("  {:<30}", r.scenario.label);
        for pp in &r.position_pnls {
            print!("  {:>14}", fmt_dollars(pp.pnl));
        }
        println!("  {:>12}{}", fmt_dollars(r.total_pnl), marker);
    }
    println!();
}

fn print_margin_summary(
    pm:   &rule4210_margin::MarginRequirement,
    regt: &rule4210_margin::MarginRequirement,
) {
    let savings  = regt.amount - pm.amount;
    let pct_save = if regt.amount > 0.0 { savings / regt.amount * 100.0 } else { 0.0 };

    println!("Margin Requirement Comparison");
    println!("  {:<42}  {:>10}", pm.method,   fmt_dollars(pm.amount));
    if let Some(ref ws) = pm.worst_scenario {
        println!("    Worst scenario: {}", ws);
    }
    println!("  {:<42}  {:>10}", regt.method, fmt_dollars(regt.amount));
    println!();
    println!("  Capital savings with Portfolio Margin:   {}  ({:.1}% reduction)",
             fmt_dollars(savings), pct_save);
    println!("  PM as % of portfolio value:              {:.1}%",
             pm.amount / (100.0 * 560.0) * 100.0);
    println!();
    println!("  ─── In production: CRR tree pricing handles American exercise ───");
    println!("  ─── and discrete dividends; vol surface replaces flat ATM vol  ───");
    println!();
}

fn fmt_dollars(v: f64) -> String {
    if v >= 0.0 {
        format!("${:>9.0}", v)
    } else {
        format!("-${:>8.0}", v.abs())
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("{}…", &s[..max - 1]) }
}
