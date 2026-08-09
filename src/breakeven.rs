//! Break-even arithmetic for deciding minimum viable trade size and edge.
//!
//! The question "what profit % is worth acting on?" has no single answer — it
//! depends on slippage tolerance, fixed costs, and crucially whether a lost
//! race costs anything. These helpers make the trade-offs explicit rather than
//! leaving them to intuition.

use crate::config::{lamports_to_sol, LAMPORTS_PER_SOL};

/// Minimum edge, as a percentage, for a trade to be worth attempting.
///
/// ```text
/// p_min = 2σ + F / (w × S)
/// ```
///
/// `2σ` because a round trip has two legs and each may fill up to the slippage
/// tolerance worse than quoted. The second term is the fixed cost spread over
/// the fraction of attempts that actually land — on the bundle path `w = 1`
/// for this purpose, since unselected bundles cost nothing.
pub fn min_edge_pct(
    slippage_bps: u16,
    fixed_cost_lamports: u64,
    win_rate: f64,
    trade_size_lamports: u64,
) -> f64 {
    let slippage_floor = 2.0 * (slippage_bps as f64 / 100.0);
    if trade_size_lamports == 0 || win_rate <= 0.0 {
        return f64::INFINITY;
    }
    let cost_pct =
        fixed_cost_lamports as f64 / (win_rate * trade_size_lamports as f64) * 100.0;
    slippage_floor + cost_pct
}

/// Minimum trade size, in lamports, for a given edge to clear costs.
///
/// ```text
/// S_min = F / (w × (p − 2σ))
/// ```
///
/// Returns `None` when the edge cannot cover slippage at any size — no amount
/// of scale rescues a trade whose margin is thinner than what slippage can
/// take.
pub fn min_size_lamports(
    edge_pct: f64,
    slippage_bps: u16,
    fixed_cost_lamports: u64,
    win_rate: f64,
) -> Option<u64> {
    let slippage_floor = 2.0 * (slippage_bps as f64 / 100.0);
    let usable = edge_pct - slippage_floor;
    if usable <= 0.0 || win_rate <= 0.0 {
        return None;
    }
    let size = fixed_cost_lamports as f64 / (win_rate * usable / 100.0);
    if !size.is_finite() {
        return None;
    }
    Some(size.round() as u64)
}

/// Render the break-even picture for the current configuration.
pub fn report(
    slippage_bps: u16,
    fixed_cost_lamports: u64,
    min_profit_pct: f64,
    uses_bundles: bool,
) -> String {
    let mut s = String::new();
    let floor = 2.0 * (slippage_bps as f64 / 100.0);

    s.push_str(&format!(
        "Slippage tolerance : {} bps per leg\n\
         Round-trip floor   : {:.3}%  (two legs, each may fill this much worse)\n\
         Fixed cost         : {} lamports ({:.9} SOL)\n\
         Submission         : {}\n\
         MIN_PROFIT         : {:.3}%\n\n",
        slippage_bps,
        floor,
        fixed_cost_lamports,
        lamports_to_sol(fixed_cost_lamports),
        if uses_bundles {
            "Jito bundle — a lost race costs NOTHING"
        } else {
            "naked transaction — every lost race burns the fee"
        },
        min_profit_pct,
    ));

    if min_profit_pct <= floor {
        s.push_str(&format!(
            "  WARNING: MIN_PROFIT ({:.3}%) does not clear the {:.3}% slippage floor.\n\
             A trade could pass the filter and still settle at a loss.\n\n",
            min_profit_pct, floor
        ));
    }

    // On the bundle path a lost race is free, so the fixed cost is only ever
    // paid on a win; the naked path spreads it over losses too.
    let win_rates: &[f64] = if uses_bundles { &[1.0] } else { &[1.0, 0.2, 0.05] };

    s.push_str("Minimum edge needed, by trade size:\n\n");
    s.push_str("  size (SOL) ");
    for w in win_rates {
        s.push_str(&format!("| win {:>4.0}% ", w * 100.0));
    }
    s.push('\n');

    for size_sol in [0.01, 0.05, 0.1, 0.5, 1.0, 5.0] {
        let size = (size_sol * LAMPORTS_PER_SOL as f64) as u64;
        s.push_str(&format!("  {:>10.2} ", size_sol));
        for w in win_rates {
            let p = min_edge_pct(slippage_bps, fixed_cost_lamports, *w, size);
            s.push_str(&format!("| {:>8.3}% ", p));
        }
        s.push('\n');
    }

    s.push_str("\nMinimum size needed, by available edge:\n\n");
    for edge in [0.1, 0.25, 0.5, 1.0, 2.0] {
        match min_size_lamports(edge, slippage_bps, fixed_cost_lamports, win_rates[0]) {
            Some(size) => s.push_str(&format!(
                "  edge {:>5.2}%  ->  {:.6} SOL\n",
                edge,
                lamports_to_sol(size)
            )),
            None => s.push_str(&format!(
                "  edge {:>5.2}%  ->  impossible: below the {:.3}% slippage floor\n",
                edge, floor
            )),
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_requirement_falls_as_size_rises() {
        // Fixed costs are size-independent, so they dominate small trades and
        // fade on large ones.
        let small = min_edge_pct(50, 20_000, 1.0, 10_000_000); // 0.01 SOL
        let large = min_edge_pct(50, 20_000, 1.0, 1_000_000_000); // 1 SOL
        assert!(small > large);
        // Both are floored by 2 x 50bps = 1%.
        assert!(large >= 1.0);
    }

    #[test]
    fn slippage_sets_an_irreducible_floor() {
        // Even at infinite size the floor remains 2x slippage.
        let huge = min_edge_pct(50, 20_000, 1.0, u64::MAX / 2);
        assert!((huge - 1.0).abs() < 0.001, "got {}", huge);
    }

    #[test]
    fn losing_races_raise_the_bar_on_the_naked_path() {
        let free_failures = min_edge_pct(50, 20_000, 1.0, 10_000_000);
        let paid_failures = min_edge_pct(50, 20_000, 0.05, 10_000_000);
        assert!(
            paid_failures > free_failures * 2.0,
            "a 5% win rate should demand a far larger edge: {} vs {}",
            paid_failures,
            free_failures
        );
    }

    #[test]
    fn edge_below_slippage_floor_is_impossible_at_any_size() {
        // 0.5% edge against a 1% floor cannot be rescued by scale.
        assert_eq!(min_size_lamports(0.5, 50, 20_000, 1.0), None);
        // Tightening slippage makes the same edge viable.
        assert!(min_size_lamports(0.5, 10, 20_000, 1.0).is_some());
    }

    #[test]
    fn min_size_shrinks_as_edge_grows() {
        let thin = min_size_lamports(1.1, 50, 20_000, 1.0).unwrap();
        let fat = min_size_lamports(2.0, 50, 20_000, 1.0).unwrap();
        assert!(fat < thin);
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        assert!(min_edge_pct(50, 20_000, 1.0, 0).is_infinite());
        assert!(min_edge_pct(50, 20_000, 0.0, 1_000).is_infinite());
        assert_eq!(min_size_lamports(1.5, 50, 20_000, 0.0), None);
    }

    #[test]
    fn report_flags_an_unsafe_threshold() {
        let out = report(50, 20_000, 0.3, true);
        assert!(out.contains("WARNING"), "got: {}", out);
        let safe = report(10, 20_000, 0.5, true);
        assert!(!safe.contains("WARNING"));
    }
}
