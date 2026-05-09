// SPDX-License-Identifier: MIT
//! RRULE ↔ Org repeater cookie conversions (Phase 17, v0.10.3).
//!
//! Spec §3.5 + §7.3.3 rule 3 — `task.repeat_rule` is canonical
//! RFC 5545 RRULE; the Org `SCHEDULED` cookie's `+1w` / `++1w` /
//! `.+1w` repeater is a best-fit projection that stock `org-agenda`
//! can render. Two helpers live here:
//!
//! - [`rrule_to_org_cookie`] — given an RRULE text + mode, emit
//!   the matching `+<N><unit>` cookie if one is expressible.
//!   Multi-weekday and BYMONTHDAY patterns degrade to "nearest
//!   interval" per spec — they emit a sensible cookie that loses
//!   the BY-clause precision, but the task isn't broken.
//!   `:RRULE:` in the properties drawer is the canonical source;
//!   the cookie is for stock `org-agenda` rendering only.
//!
//! - [`org_repeater_to_rrule`] — given an [`OrgRepeater`] cookie,
//!   reconstruct the RRULE the cookie *implies* (FREQ + INTERVAL,
//!   nothing else). Used by divergence detection on read-back: if
//!   the file has both a SCHEDULED cookie and a `:RRULE:`
//!   property, and the cookie's reconstructed RRULE doesn't match
//!   the stored one, the user has edited the cookie in Emacs and
//!   we surface the divergence + rewrite the cookie to match
//!   `:RRULE:` (DB stays canonical).

use atrium_core::repeat::RepeatMode;

use crate::org::OrgRepeater;

/// Map an RRULE text to the best-fit Org cookie. Returns the
/// cookie body (e.g. `"+1w"` / `"++3d"` / `".+1m"`) without the
/// surrounding angle brackets — the caller embeds it into the
/// `<DATE COOKIE>` form.
///
/// Returns `None` only when the RRULE has no recognisable `FREQ=`
/// (malformed). BY-clauses lose precision (`BYDAY=MO,WE` →
/// just `+1w`); the canonical pattern stays in the `:RRULE:`
/// property where Atrium re-parses it on read-back.
pub fn rrule_to_org_cookie(rrule_text: &str, mode: RepeatMode) -> Option<String> {
    let (freq, interval) = parse_freq_and_interval(rrule_text)?;
    let unit = freq_to_unit(&freq)?;
    let prefix = mode.org_cookie();
    Some(format!("{prefix}{interval}{unit}"))
}

/// Map an Org repeater cookie back to the RRULE it implies. Used
/// by [`crate::vault_watcher`] to detect divergence between the
/// SCHEDULED cookie and the `:RRULE:` property on read-back.
///
/// Cookies only express FREQ + INTERVAL; the returned string is
/// always one of those two clauses (`FREQ=DAILY;INTERVAL=3`).
/// Returns `None` if the unit isn't one of `d` / `w` / `m` / `y`.
pub fn org_repeater_to_rrule(repeater: &OrgRepeater) -> Option<String> {
    let freq = match repeater.unit {
        'd' | 'D' => "DAILY",
        'w' | 'W' => "WEEKLY",
        'm' | 'M' => "MONTHLY",
        'y' | 'Y' => "YEARLY",
        _ => return None,
    };
    if repeater.interval == 1 {
        Some(format!("FREQ={freq}"))
    } else {
        Some(format!("FREQ={freq};INTERVAL={}", repeater.interval))
    }
}

/// True when the cookie-implied RRULE matches the stored RRULE on
/// the FREQ + INTERVAL axis. BY-clauses in the stored RRULE don't
/// count as divergence — the cookie can't express them by design;
/// it's still consistent with the stored rule's frequency. Useful
/// for the divergence detector: only flag as diverged when the
/// user actually changed the cookie's frequency / interval (e.g.
/// `+1w` → `+2w` in Emacs), not just because the stored rule has
/// `BYDAY=MO,WE`.
pub fn cookie_matches_rrule(repeater: &OrgRepeater, rrule_text: &str) -> bool {
    let Some((freq, interval)) = parse_freq_and_interval(rrule_text) else {
        return false;
    };
    let Some(unit) = freq_to_unit(&freq) else {
        return false;
    };
    let cookie_unit = repeater.unit.to_ascii_lowercase();
    cookie_unit == unit && interval == repeater.interval
}

fn parse_freq_and_interval(rrule_text: &str) -> Option<(String, u32)> {
    let mut freq: Option<String> = None;
    let mut interval: u32 = 1;
    for token in rrule_text.split(';') {
        let trimmed = token.trim();
        let Some((k, v)) = trimmed.split_once('=') else {
            continue;
        };
        match k.trim().to_ascii_uppercase().as_str() {
            "FREQ" => freq = Some(v.trim().to_ascii_uppercase()),
            "INTERVAL" => interval = v.trim().parse().ok()?,
            _ => {}
        }
    }
    Some((freq?, interval))
}

fn freq_to_unit(freq: &str) -> Option<char> {
    match freq {
        "DAILY" => Some('d'),
        "WEEKLY" => Some('w'),
        "MONTHLY" => Some('m'),
        "YEARLY" => Some('y'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(prefix: &str, interval: u32, unit: char) -> OrgRepeater {
        OrgRepeater {
            mode: prefix.to_string(),
            interval,
            unit,
        }
    }

    // ── rrule_to_org_cookie ──────────────────────────────────

    #[test]
    fn weekly_single_day_emits_plus1w() {
        // BYDAY=SU is lossless when SCHEDULED lands on a Sunday;
        // either way the cookie is `+1w`.
        assert_eq!(
            rrule_to_org_cookie("FREQ=WEEKLY;BYDAY=SU", RepeatMode::Basic),
            Some("+1w".to_string())
        );
    }

    #[test]
    fn weekly_multi_day_degrades_to_interval() {
        // Multi-weekday is the canonical "lossy cookie" case.
        // Spec §7.3.3 rule 3: emit best-fit; org-agenda shows the
        // wrong frequency but the task isn't broken.
        assert_eq!(
            rrule_to_org_cookie("FREQ=WEEKLY;BYDAY=MO,WE", RepeatMode::Cumulative),
            Some("++1w".to_string())
        );
    }

    #[test]
    fn monthly_bymonthday_degrades_to_interval() {
        assert_eq!(
            rrule_to_org_cookie("FREQ=MONTHLY;BYMONTHDAY=1", RepeatMode::Next),
            Some(".+1m".to_string())
        );
    }

    #[test]
    fn daily_with_interval() {
        assert_eq!(
            rrule_to_org_cookie("FREQ=DAILY;INTERVAL=3", RepeatMode::Cumulative),
            Some("++3d".to_string())
        );
    }

    #[test]
    fn weekly_default_interval() {
        assert_eq!(
            rrule_to_org_cookie("FREQ=WEEKLY", RepeatMode::Cumulative),
            Some("++1w".to_string())
        );
    }

    #[test]
    fn yearly_works() {
        assert_eq!(
            rrule_to_org_cookie("FREQ=YEARLY", RepeatMode::Basic),
            Some("+1y".to_string())
        );
    }

    #[test]
    fn missing_freq_returns_none() {
        assert_eq!(rrule_to_org_cookie("INTERVAL=3", RepeatMode::Basic), None);
    }

    #[test]
    fn unrecognised_freq_returns_none() {
        assert_eq!(rrule_to_org_cookie("FREQ=NEVER", RepeatMode::Basic), None);
    }

    #[test]
    fn case_insensitive_keys() {
        assert_eq!(
            rrule_to_org_cookie("freq=daily;interval=2", RepeatMode::Basic),
            Some("+2d".to_string())
        );
    }

    // ── org_repeater_to_rrule ────────────────────────────────

    #[test]
    fn cookie_to_rrule_drops_interval_when_one() {
        let r = cookie("+", 1, 'w');
        assert_eq!(org_repeater_to_rrule(&r), Some("FREQ=WEEKLY".to_string()));
    }

    #[test]
    fn cookie_to_rrule_keeps_interval_when_above_one() {
        let r = cookie("+", 3, 'd');
        assert_eq!(
            org_repeater_to_rrule(&r),
            Some("FREQ=DAILY;INTERVAL=3".to_string())
        );
    }

    #[test]
    fn cookie_to_rrule_handles_all_modes() {
        // The mode prefix doesn't change the implied FREQ.
        for prefix in &["+", "++", ".+"] {
            let r = cookie(prefix, 1, 'm');
            assert_eq!(
                org_repeater_to_rrule(&r),
                Some("FREQ=MONTHLY".to_string()),
                "prefix: {prefix}"
            );
        }
    }

    // ── cookie_matches_rrule ─────────────────────────────────

    #[test]
    fn cookie_matches_simple_rrule() {
        let r = cookie("+", 1, 'w');
        assert!(cookie_matches_rrule(&r, "FREQ=WEEKLY"));
    }

    #[test]
    fn cookie_matches_rrule_with_byday_clause() {
        // BY-clauses don't count as divergence — they're outside
        // what the cookie can express anyway.
        let r = cookie("+", 1, 'w');
        assert!(cookie_matches_rrule(&r, "FREQ=WEEKLY;BYDAY=MO,WE"));
    }

    #[test]
    fn cookie_disagrees_when_interval_differs() {
        let r = cookie("+", 2, 'w');
        assert!(!cookie_matches_rrule(&r, "FREQ=WEEKLY"));
    }

    #[test]
    fn cookie_disagrees_when_unit_differs() {
        let r = cookie("+", 1, 'w');
        assert!(!cookie_matches_rrule(&r, "FREQ=DAILY"));
    }
}
