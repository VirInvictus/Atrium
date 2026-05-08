// SPDX-License-Identifier: MIT
//! Ranking helpers — bare-text term extraction + relevance blend.
//!
//! Bare-text searches in Atrium have historically come back in
//! `task.position` order, which is the right answer for *list* views
//! but misses the "what looks like the closest match to what I typed"
//! signal that any modern search expects. v0.6.x ranks bare-text
//! results by FTS5 `bm25` blended with a recency factor:
//!
//! ```text
//!     score = (|bm25| / (1 + |bm25|)) + recency_weight · 2^(-Δd / H)
//! ```
//!
//! - `bm25` — FTS5's `bm25(task_fts)` (more negative = more
//!   relevant). The relevance term is the *saturating* mapping
//!   `|bm25| / (1 + |bm25|)`: zero stays zero, and as `|bm25|`
//!   grows the term asymptotes at 1.0. That keeps the relevance
//!   contribution on a stable [0, 1) scale regardless of the
//!   absolute magnitudes FTS5 happens to produce on a given DB.
//! - `Δd` — days since the task was last modified.
//! - `H` — half-life, defaults to 30 days. After H days the recency
//!   contribution is halved; after 2H it's quartered. Keeps the
//!   ranking stable for old well-matched tasks while letting freshly
//!   touched ones edge out lukewarm matches.
//! - `recency_weight` — fixed 0.25. The relevance term dominates;
//!   recency is a tiebreaker, not the primary signal.
//!
//! Both helpers in this module are pure: zero DB access, no IO,
//! deterministic. The DB-side `bm25_for_terms` lookup lives in
//! `atrium-core::db::read` because only that crate touches SQLite.

use crate::ast::Expr;

/// Collect every bare-text term (`Expr::Text`) reachable from the
/// expression. Used to decide whether the bm25 fast-path applies and,
/// if so, what string to feed the FTS5 `MATCH` clause.
///
/// Field-scoped operators like `title:milk` are *not* included —
/// they're already in-engine substring/exact matches. Only the
/// "freeform top-level word" form qualifies. This matches the
/// spec §4.3 framing of bare text as "the FTS5 column".
pub fn collect_text_terms(expr: &Expr) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    walk(expr, &mut out);
    out
}

fn walk(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Text(s) => {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                out.push(trimmed.to_string());
            }
        }
        Expr::And(items) | Expr::Or(items) => {
            for item in items {
                walk(item, out);
            }
        }
        Expr::Not(inner) => walk(inner, out),
        // Field/Compare/Range/State/Pass do not contribute bare text.
        // A `title:` Text MatchKind is a column-scoped match, not
        // freeform; bm25 doesn't apply to it any differently than
        // the in-memory substring. Keep the fast-path narrow.
        Expr::Field { .. } => {
            // Field-scoped operators (title:, tag:, …) live in the
            // in-memory evaluator; bm25's column-weighting doesn't
            // give us anything extra over the substring/exact path
            // there. Keep the fast-path narrow.
        }
        Expr::Compare { .. } | Expr::Range { .. } | Expr::State(_) | Expr::Pass => {}
    }
}

/// Blend FTS5 bm25 with a recency factor.
///
/// `bm25` is FTS5's raw score (negative; smaller = more relevant —
/// FTS5 returns it that way so `ORDER BY rank` works without a
/// `DESC` flip). Pass it through as-is; the formula handles the
/// inversion.
///
/// `days_since_modified` is the integer-day delta `today -
/// task.modified_at::date`. Negative deltas (clock skew, future-
/// dated tasks) clamp to 0.
///
/// `half_life_days` controls how fast the recency contribution
/// decays — at `half_life_days` the factor is 0.5; at `2 *
/// half_life_days` it's 0.25; etc. Pass a sensible positive value
/// (the call site uses 30.0).
pub fn blend_relevance(bm25: f64, days_since_modified: i64, half_life_days: f64) -> f64 {
    // bm25 is negative-or-zero (FTS5 convention: smaller = more
    // relevant). Take |bm25| as the "relevance magnitude", then map
    // [0, ∞) → [0, 1) via the saturating function x / (1 + x). At
    // bm25 = 0 the relevance is 0; at very strong matches it
    // approaches 1.0 without ever reaching it. Keeps the relevance
    // term on a stable scale regardless of FTS5's per-DB magnitudes.
    let abs_bm25 = (-bm25).max(0.0);
    let relevance = abs_bm25 / (1.0 + abs_bm25);

    let days = days_since_modified.max(0) as f64;
    let half_life = half_life_days.max(0.001); // guard against / by 0
    // 2^(-days / half_life). Pure exponential decay.
    let recency = 0.5_f64.powf(days / half_life);

    let recency_weight = 0.25;
    relevance + recency_weight * recency
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Comparator, Field, MatchKind, State, Value};

    fn text(s: &str) -> Expr {
        Expr::Text(s.into())
    }

    #[test]
    fn collect_terms_empty_for_state_only() {
        let expr = Expr::State(State::Open);
        assert!(collect_text_terms(&expr).is_empty());
    }

    #[test]
    fn collect_terms_returns_single_bareword() {
        let expr = text("milk");
        assert_eq!(collect_text_terms(&expr), vec!["milk".to_string()]);
    }

    #[test]
    fn collect_terms_walks_and() {
        let expr = Expr::And(vec![text("milk"), Expr::State(State::Open), text("eggs")]);
        assert_eq!(
            collect_text_terms(&expr),
            vec!["milk".to_string(), "eggs".to_string()]
        );
    }

    #[test]
    fn collect_terms_walks_or_and_not() {
        let expr = Expr::Or(vec![
            Expr::Not(Box::new(text("milk"))),
            Expr::And(vec![text("eggs"), text("flour")]),
        ]);
        assert_eq!(
            collect_text_terms(&expr),
            vec!["milk".to_string(), "eggs".to_string(), "flour".to_string()]
        );
    }

    #[test]
    fn collect_terms_skips_field_scoped() {
        // `title:bread` is a column-scoped match, not bare text;
        // bm25 already factors title weight into its own column
        // weights. Keep the fast-path narrow.
        let expr = Expr::Field {
            field: Field::Title,
            kind: MatchKind::Substring("bread".into()),
        };
        assert!(collect_text_terms(&expr).is_empty());
    }

    #[test]
    fn collect_terms_skips_compare_and_range() {
        let expr = Expr::And(vec![
            Expr::Compare {
                field: Field::Estimated,
                comp: Comparator::Lt,
                value: Value::Number(30),
            },
            Expr::Range {
                field: Field::Due,
                low: Value::Date(chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()),
                high: Value::Date(chrono::NaiveDate::from_ymd_opt(2026, 5, 31).unwrap()),
            },
        ]);
        assert!(collect_text_terms(&expr).is_empty());
    }

    #[test]
    fn collect_terms_drops_empty_strings() {
        let expr = Expr::And(vec![text("   "), text("milk")]);
        assert_eq!(collect_text_terms(&expr), vec!["milk".to_string()]);
    }

    #[test]
    fn blend_recent_match_outranks_stale_match_at_equal_relevance() {
        let bm25 = -3.0;
        let recent = blend_relevance(bm25, 0, 30.0);
        let stale = blend_relevance(bm25, 60, 30.0);
        assert!(
            recent > stale,
            "recent ({recent}) should outrank stale ({stale}) at same bm25"
        );
    }

    #[test]
    fn blend_strong_match_outranks_weak_match_at_equal_recency() {
        let strong = blend_relevance(-10.0, 5, 30.0);
        let weak = blend_relevance(-1.0, 5, 30.0);
        assert!(
            strong > weak,
            "strong bm25 ({strong}) should outrank weak ({weak}) at same recency"
        );
    }

    #[test]
    fn blend_clamps_negative_days() {
        // Future-dated modified_at (clock skew) shouldn't blow up.
        let a = blend_relevance(-3.0, -10, 30.0);
        let b = blend_relevance(-3.0, 0, 30.0);
        assert!((a - b).abs() < 1e-9, "negative days clamp to 0");
    }

    #[test]
    fn blend_half_life_halves_recency_contribution() {
        // At half-life days, recency = 0.5. Pin relevance to 0 by
        // using bm25 = 0 so the only score contribution is the
        // recency term, then check the math at days=0 and days=H.
        let bm25 = 0.0; // relevance term collapses to 0
        let now = blend_relevance(bm25, 0, 30.0); // 0 + 0.25 * 1.0 = 0.25
        let half = blend_relevance(bm25, 30, 30.0); // 0 + 0.25 * 0.5 = 0.125
        assert!((now - 0.25).abs() < 1e-9);
        assert!((half - 0.125).abs() < 1e-9);
    }

    #[test]
    fn blend_handles_zero_bm25() {
        // bm25 = 0 means a perfect-or-no match (FTS5 returns 0 for
        // matches that match nothing of significance). Don't divide
        // by zero or produce NaN.
        let s = blend_relevance(0.0, 0, 30.0);
        assert!(s.is_finite());
        assert!(s > 0.0);
    }
}
