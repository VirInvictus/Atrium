// SPDX-License-Identifier: MIT
//! `ScheduledFor` — task's `scheduled_for` field is either a specific
//! date OR the literal `__someday__` sentinel (spec §4.2). This enum
//! encodes that contract in Rust so the type system enforces what the
//! schema's CHECK could not.

use std::fmt;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

const SOMEDAY_SENTINEL: &str = "__someday__";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScheduledFor {
    /// The "Someday" list — see spec §4.2.
    Someday,
    /// Specific calendar date.
    Date(NaiveDate),
}

impl ScheduledFor {
    /// Parse from the schema's TEXT representation.
    pub fn parse(s: &str) -> Option<Self> {
        if s == SOMEDAY_SENTINEL {
            Some(Self::Someday)
        } else {
            NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .ok()
                .map(Self::Date)
        }
    }
}

impl fmt::Display for ScheduledFor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Someday => f.write_str(SOMEDAY_SENTINEL),
            Self::Date(d) => write!(f, "{}", d.format("%Y-%m-%d")),
        }
    }
}

impl rusqlite::ToSql for ScheduledFor {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for ScheduledFor {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = value.as_str()?;
        Self::parse(s).ok_or_else(|| {
            rusqlite::types::FromSqlError::Other(format!("invalid scheduled_for value: {s}").into())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_someday_sentinel() {
        assert_eq!(
            ScheduledFor::parse("__someday__"),
            Some(ScheduledFor::Someday)
        );
    }

    #[test]
    fn parse_iso_date() {
        let d = NaiveDate::from_ymd_opt(2026, 5, 15).unwrap();
        assert_eq!(
            ScheduledFor::parse("2026-05-15"),
            Some(ScheduledFor::Date(d))
        );
    }

    #[test]
    fn parse_rejects_garbage() {
        assert_eq!(ScheduledFor::parse("not-a-date"), None);
    }

    #[test]
    fn display_round_trips() {
        let d = ScheduledFor::Date(NaiveDate::from_ymd_opt(2026, 5, 15).unwrap());
        assert_eq!(d.to_string(), "2026-05-15");
        assert_eq!(ScheduledFor::Someday.to_string(), "__someday__");
    }
}
