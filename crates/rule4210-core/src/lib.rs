// rule4210-core — shared domain types
//
// All other crates in the workspace depend on this one.  Keep it
// dependency-light: only serde for serialisation.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Instrument
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OptionType {
    Call,
    Put,
}

impl std::fmt::Display for OptionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OptionType::Call => write!(f, "Call"),
            OptionType::Put  => write!(f, "Put"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExerciseStyle {
    American,
    European,
}

/// A vanilla equity option contract (100-share multiplier by convention).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EquityOption {
    pub underlying:       String,
    pub strike:           f64,
    /// ISO-8601 date string, e.g. "2026-04-17"
    pub expiry:           String,
    /// Time to expiry in years, pre-computed at position creation.
    pub time_to_expiry:   f64,
    pub option_type:      OptionType,
    pub exercise_style:   ExerciseStyle,
    /// Contract multiplier — 100 for standard US equity options.
    pub multiplier:       f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Instrument {
    Stock  { symbol: String },
    Option(EquityOption),
}

impl Instrument {
    pub fn underlying(&self) -> &str {
        match self {
            Instrument::Stock  { symbol }  => symbol,
            Instrument::Option(o)          => &o.underlying,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Instrument::Stock  { symbol }  => symbol.clone(),
            Instrument::Option(o)          => format!(
                "{} {} {} {:.0}",
                o.underlying, o.expiry, o.option_type, o.strike
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Position
// ---------------------------------------------------------------------------

/// A single line in a portfolio.
///
/// `quantity` is signed: positive = long, negative = short.
/// For options, quantity is measured in contracts (each = 100 shares).
/// `cost_basis` is per share / per share-equivalent (not multiplied).
#[derive(Debug, Clone)]
pub struct Position {
    pub instrument: Instrument,
    pub quantity:   f64,
    pub cost_basis: f64,
}

impl Position {
    pub fn new(instrument: Instrument, quantity: f64, cost_basis: f64) -> Self {
        Self { instrument, quantity, cost_basis }
    }
}

// ---------------------------------------------------------------------------
// Portfolio
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Portfolio {
    pub name:      String,
    pub positions: Vec<Position>,
}

impl Portfolio {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), positions: Vec::new() }
    }

    pub fn add(&mut self, p: Position) {
        self.positions.push(p);
    }

    /// Group positions by underlying symbol for margin calculation.
    /// Each group becomes one margin unit under TIMS.
    pub fn margin_groups(&self) -> HashMap<String, Vec<&Position>> {
        let mut map: HashMap<String, Vec<&Position>> = HashMap::new();
        for p in &self.positions {
            map.entry(p.instrument.underlying().to_string())
               .or_default()
               .push(p);
        }
        map
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn spy_stock() -> Position {
        Position::new(
            Instrument::Stock { symbol: "SPY".to_string() },
            100.0,
            560.0,
        )
    }

    fn spy_put() -> Position {
        Position::new(
            Instrument::Option(EquityOption {
                underlying:     "SPY".to_string(),
                strike:         540.0,
                expiry:         "2026-04-17".to_string(),
                time_to_expiry: 30.0 / 365.0,
                option_type:    OptionType::Put,
                exercise_style: ExerciseStyle::American,
                multiplier:     100.0,
            }),
            1.0,
            3.81,
        )
    }

    #[test]
    fn instrument_underlying() {
        assert_eq!(spy_stock().instrument.underlying(), "SPY");
        assert_eq!(spy_put().instrument.underlying(), "SPY");
    }

    #[test]
    fn portfolio_margin_groups_single_underlying() {
        let mut port = Portfolio::new("test");
        port.add(spy_stock());
        port.add(spy_put());
        let groups = port.margin_groups();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups["SPY"].len(), 2);
    }

    #[test]
    fn portfolio_margin_groups_two_underlyings() {
        let mut port = Portfolio::new("test");
        port.add(spy_stock());
        port.add(Position::new(
            Instrument::Stock { symbol: "QQQ".to_string() },
            50.0,
            480.0,
        ));
        let groups = port.margin_groups();
        assert_eq!(groups.len(), 2);
    }

    #[test]
    fn option_label_format() {
        let label = spy_put().instrument.label();
        assert!(label.contains("SPY"));
        assert!(label.contains("540"));
        assert!(label.contains("Put"));
    }

    #[test]
    fn position_quantity_sign_convention() {
        let long  = Position::new(Instrument::Stock { symbol: "SPY".to_string() },  100.0, 560.0);
        let short = Position::new(Instrument::Stock { symbol: "SPY".to_string() }, -100.0, 560.0);
        assert!(long.quantity  > 0.0);
        assert!(short.quantity < 0.0);
    }
}
