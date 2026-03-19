// rule4210-server — Axum HTTP server
//
// Routes:
//   GET  /                → embedded HTML frontend
//   GET  /health          → liveness probe
//   GET  /api/demo        → pre-loaded SPY collar result (no body needed)
//   POST /api/margin      → calculate PM for an arbitrary portfolio (JSON body)

use axum::{
    extract::Json,
    http::{HeaderValue, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use rule4210_core::{EquityOption, ExerciseStyle, Instrument, OptionType, Position};
use rule4210_margin::{portfolio_margin, reg_t_margin};
use rule4210_pricer::{BlackScholesPricer, CrrPricer, MarketContext};
use rule4210_scenarios::{tims_scenarios, ScenarioEngine};

// ── Request types ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct MarginRequest {
    underlying: String,
    spot:       f64,
    /// Decimal fraction, e.g. 0.172 for 17.2%
    vol:        f64,
    rate:       f64,
    div_yield:  f64,
    positions:  Vec<PosReq>,
    /// "bs" (default) or "crr"
    #[serde(default = "default_pricer")]
    pricer:     String,
}

fn default_pricer() -> String { "bs".into() }

#[derive(Deserialize)]
struct PosReq {
    /// "stock" | "call" | "put"
    kind:   String,
    qty:    f64,
    // option-only fields
    strike: Option<f64>,
    expiry: Option<String>,
    /// Time to expiry in years, computed by the frontend from the expiry date.
    tte:    Option<f64>,
}

// ── Response types ───────────────────────────────────────────────────────────

#[derive(Serialize)]
struct MarginResponse {
    position_labels:  Vec<String>,
    scenarios:        Vec<ScenarioResp>,
    portfolio_margin: f64,
    reg_t:            f64,
    savings:          f64,
    savings_pct:      f64,
    worst_scenario:   String,
    portfolio_value:  f64,
}

#[derive(Serialize)]
struct ScenarioResp {
    label:         String,
    position_pnls: Vec<f64>,
    total_pnl:     f64,
    is_worst:      bool,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn serve_index() -> Response {
    let mut resp = Html(include_str!("../static/index.html")).into_response();
    resp.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store, must-revalidate"),
    );
    resp
}

async fn health() -> &'static str { "ok" }

async fn demo_handler() -> ApiResult<MarginResponse> {
    run_margin(default_spy_collar())
}

async fn calculate_handler(Json(req): Json<MarginRequest>) -> ApiResult<MarginResponse> {
    run_margin(req)
}

// ── Default portfolio ─────────────────────────────────────────────────────────

fn default_spy_collar() -> MarginRequest {
    MarginRequest {
        underlying: "SPY".into(),
        spot:       560.0,
        vol:        0.172,
        rate:       0.045,
        div_yield:  0.013,
        pricer:     "bs".into(),
        positions: vec![
            PosReq { kind: "stock".into(), qty: 100.0,
                     strike: None, expiry: None, tte: None },
            PosReq { kind: "put".into(),   qty: 1.0,  strike: Some(540.0),
                     expiry: Some("2026-04-17".into()), tte: Some(30.0 / 365.0) },
            PosReq { kind: "call".into(),  qty: -1.0, strike: Some(580.0),
                     expiry: Some("2026-04-17".into()), tte: Some(30.0 / 365.0) },
        ],
    }
}

// ── Core calculation ──────────────────────────────────────────────────────────

fn run_margin(req: MarginRequest) -> ApiResult<MarginResponse> {
    let positions = build_positions(&req)?;
    let pos_refs: Vec<&Position> = positions.iter().collect();

    let ctx = MarketContext {
        spot:      req.spot,
        vol:       req.vol,
        rate:      req.rate,
        div_yield: req.div_yield,
    };

    let bs  = BlackScholesPricer;
    let crr = CrrPricer;
    let pricer_ref: &dyn rule4210_pricer::Pricer = if req.pricer == "crr" { &crr } else { &bs };
    let engine    = ScenarioEngine::new(pricer_ref);
    let scenarios = tims_scenarios();
    let results   = engine.run(&pos_refs, &ctx, &scenarios);

    let worst_pnl = results.iter().map(|r| r.total_pnl).fold(f64::INFINITY, f64::min);

    let scenario_resps: Vec<ScenarioResp> = results.iter().map(|r| ScenarioResp {
        label:         r.scenario.label.clone(),
        position_pnls: r.position_pnls.iter().map(|pp| pp.pnl).collect(),
        total_pnl:     r.total_pnl,
        is_worst:      (r.total_pnl - worst_pnl).abs() < 0.01,
    }).collect();

    let n_contracts = positions.iter()
        .filter(|p| matches!(p.instrument, Instrument::Option(_)))
        .count();

    let pm   = portfolio_margin(&results, n_contracts);
    let regt = reg_t_margin(&pos_refs, req.spot);

    let savings     = regt.amount - pm.amount;
    let savings_pct = if regt.amount > 0.0 { savings / regt.amount * 100.0 } else { 0.0 };
    let portfolio_value: f64 = positions.iter()
        .filter(|p| matches!(p.instrument, Instrument::Stock { .. }))
        .map(|p| p.quantity * req.spot)
        .sum();

    Ok(Json(MarginResponse {
        position_labels:  positions.iter().map(|p| p.instrument.label()).collect(),
        scenarios:        scenario_resps,
        portfolio_margin: pm.amount,
        reg_t:            regt.amount,
        savings,
        savings_pct,
        worst_scenario:   pm.worst_scenario.unwrap_or_default(),
        portfolio_value,
    }))
}

fn build_positions(req: &MarginRequest) -> Result<Vec<Position>, (StatusCode, String)> {
    let mut out = Vec::new();
    for p in &req.positions {
        let instrument = match p.kind.as_str() {
            "stock" => Instrument::Stock { symbol: req.underlying.clone() },
            "call" | "put" => {
                let strike = p.strike.ok_or((StatusCode::BAD_REQUEST, "option needs strike".into()))?;
                let expiry = p.expiry.clone().ok_or((StatusCode::BAD_REQUEST, "option needs expiry".into()))?;
                let tte    = p.tte.ok_or((StatusCode::BAD_REQUEST, "option needs tte".into()))?;
                if tte <= 0.0 {
                    return Err((StatusCode::BAD_REQUEST, "tte must be > 0".into()));
                }
                Instrument::Option(EquityOption {
                    underlying:     req.underlying.clone(),
                    strike,
                    expiry,
                    time_to_expiry: tte,
                    option_type:    if p.kind == "call" { OptionType::Call } else { OptionType::Put },
                    exercise_style: ExerciseStyle::European,
                    multiplier:     100.0,
                })
            }
            other => return Err((StatusCode::BAD_REQUEST, format!("unknown kind: {other}"))),
        };
        out.push(Position::new(instrument, p.qty, 0.0));
    }
    Ok(out)
}

// ── Server startup ────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/",            get(serve_index))
        .route("/health",      get(health))
        .route("/api/demo",    get(demo_handler))
        .route("/api/margin",  post(calculate_handler))
        .layer(CorsLayer::permissive());

    let addr = "0.0.0.0:8080";
    println!("Rule 4210 server → http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn collar_result() -> MarginResponse {
        run_margin(default_spy_collar()).unwrap().0
    }

    #[test]
    fn demo_returns_twelve_scenarios() {
        assert_eq!(collar_result().scenarios.len(), 12);
    }

    #[test]
    fn pm_less_than_regt_for_hedged_collar() {
        let r = collar_result();
        assert!(r.portfolio_margin < r.reg_t,
            "PM={} should be < Reg-T={}", r.portfolio_margin, r.reg_t);
    }

    #[test]
    fn savings_positive() {
        let r = collar_result();
        assert!(r.savings > 0.0, "savings={}", r.savings);
        assert!(r.savings_pct > 80.0, "expected >80% saving, got {:.1}%", r.savings_pct);
    }

    #[test]
    fn exactly_one_worst_scenario() {
        let r = collar_result();
        let worst_count = r.scenarios.iter().filter(|s| s.is_worst).count();
        assert_eq!(worst_count, 1, "expected exactly 1 worst scenario");
    }

    #[test]
    fn position_pnl_count_matches_positions() {
        let r = collar_result();
        let n_pos = r.position_labels.len();
        for s in &r.scenarios {
            assert_eq!(s.position_pnls.len(), n_pos);
        }
    }

    #[test]
    fn bad_kind_returns_error() {
        let mut req = default_spy_collar();
        req.positions[0].kind = "future".into();
        assert!(run_margin(req).is_err());
    }

    #[test]
    fn option_missing_strike_returns_error() {
        let mut req = default_spy_collar();
        req.positions[1].strike = None;
        assert!(run_margin(req).is_err());
    }
}
