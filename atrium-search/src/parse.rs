// SPDX-License-Identifier: MIT
//! Recursive-descent parser. Tokens → [`Expr`] AST.
//!
//! Grammar (precedence: NOT > AND > OR; matches Calibre, SQL, Python):
//!
//! ```text
//! expr    = or_expr
//! or_expr = and_expr ( "OR" and_expr )*
//! and_expr = not_expr ( ("AND")? not_expr )*       (implicit AND)
//! not_expr = ( "NOT" | "!" ) not_expr | primary
//! primary = "(" or_expr ")"
//!         | field ":" value_or_match
//!         | bareword                                (freeform text)
//!         | quoted_string                           (freeform text)
//! ```
//!
//! `field:` parses where `field` is one of the recognised names
//! (case-insensitive). Unknown field names fall through to freeform
//! text — `tga:foo` becomes `Text("tga:foo")` plus a parse warning
//! the caller can surface.

use chrono::NaiveDate;

use super::ast::{
    Comparator, DateKeyword, Expr, Field, MatchKind, SortDirection, SortKey, SortSpec, State, Value,
};
use super::lex::{Token, lex};

/// Output of a successful parse, plus any lossy-but-non-fatal
/// warnings (unknown field names that fell through to freeform).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseResult {
    pub expr: Expr,
    pub warnings: Vec<String>,
    /// v0.4.1 — sort modifiers extracted from the input. Each
    /// `sort:KEY` token in the source produces one entry here, in
    /// input order (primary → secondary). The parser also strips
    /// these from the AST (replaces them with `Expr::Pass`) so the
    /// evaluator never sees a sort modifier as a predicate.
    pub sorts: Vec<SortSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Empty input.
    Empty,
    /// Mismatched parens.
    UnbalancedParens,
    /// `..` with no left or right operand.
    BadRange,
    /// Comparison operator with no value.
    BadComparison,
    /// Generic syntax error with a token-position hint.
    Syntax(String),
}

/// Parse a search expression. Returns the AST + non-fatal warnings;
/// `Err` only on hard syntax failures (empty input, unbalanced
/// parens, etc.). Unknown field names are warnings, not errors.
pub fn parse(input: &str) -> Result<ParseResult, ParseError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(ParseError::Empty);
    }
    let tokens = lex(trimmed);
    if tokens.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut p = Parser {
        tokens,
        pos: 0,
        warnings: Vec::new(),
        sorts: Vec::new(),
    };
    let expr = p.parse_or()?;
    if p.pos < p.tokens.len() {
        return Err(ParseError::Syntax(format!(
            "trailing tokens at position {}",
            p.pos
        )));
    }
    Ok(ParseResult {
        expr,
        warnings: p.warnings,
        sorts: p.sorts,
    })
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    warnings: Vec<String>,
    sorts: Vec<SortSpec>,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn bump(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    fn eat_keyword(&mut self, word: &str) -> bool {
        if let Some(Token::Bareword(b)) = self.peek()
            && b.eq_ignore_ascii_case(word)
        {
            self.pos += 1;
            return true;
        }
        false
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut items = vec![self.parse_and()?];
        while self.eat_keyword("OR") {
            items.push(self.parse_and()?);
        }
        Ok(if items.len() == 1 {
            items.pop().unwrap()
        } else {
            Expr::Or(items)
        })
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut items = vec![self.parse_not()?];
        loop {
            // Stop conditions: end of stream, RParen, an OR keyword
            // (the parent level handles it).
            match self.peek() {
                None | Some(Token::RParen) => break,
                Some(Token::Bareword(b)) if b.eq_ignore_ascii_case("OR") => break,
                _ => {}
            }
            // Explicit AND or implicit AND (next primary).
            self.eat_keyword("AND");
            items.push(self.parse_not()?);
        }
        Ok(if items.len() == 1 {
            items.pop().unwrap()
        } else {
            Expr::And(items)
        })
    }

    fn parse_not(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Token::Bang) => {
                self.bump();
                Ok(Expr::Not(Box::new(self.parse_not()?)))
            }
            Some(Token::Bareword(b)) if b.eq_ignore_ascii_case("NOT") => {
                self.bump();
                Ok(Expr::Not(Box::new(self.parse_not()?)))
            }
            _ => self.parse_primary(),
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            None => Err(ParseError::Syntax("expected expression".into())),
            Some(Token::LParen) => {
                self.bump();
                let inner = self.parse_or()?;
                match self.bump() {
                    Some(Token::RParen) => Ok(inner),
                    _ => Err(ParseError::UnbalancedParens),
                }
            }
            Some(Token::Bareword(_) | Token::QuotedString(_)) => self.parse_term(),
            Some(other) => Err(ParseError::Syntax(format!("unexpected token {other:?}"))),
        }
    }

    /// A "term" is either a `field:value` expression, an `is:STATE`
    /// state predicate, or a bare/quoted text token.
    fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let head = self.bump().expect("checked by parse_primary");
        let head_text = match &head {
            Token::Bareword(s) => s.clone(),
            Token::QuotedString(s) => s.clone(),
            _ => unreachable!(),
        };
        let head_was_quoted = matches!(head, Token::QuotedString(_));
        // Is this a `field:` expression?
        if !head_was_quoted && matches!(self.peek(), Some(Token::Colon)) {
            // Eat the colon, then parse the right-hand side based on
            // whether `head_text` is a recognised field name.
            self.bump();
            return self.parse_field_value(&head_text);
        }
        // Otherwise it's freeform text.
        Ok(Expr::Text(head_text))
    }

    fn parse_field_value(&mut self, field_name: &str) -> Result<Expr, ParseError> {
        // Special: `is:NAME` is a state predicate.
        if field_name.eq_ignore_ascii_case("is") {
            return self.parse_state_predicate();
        }
        // v0.4.1 — `sort:KEY` / `sort:-KEY` is a sort modifier, not a
        // predicate. Capture it onto self.sorts and return Expr::Pass
        // so And/Or composition treats it as identity. Unknown keys
        // become a warning (same shape as unknown fields) and the
        // token falls through to freeform text.
        if field_name.eq_ignore_ascii_case("sort") {
            return self.parse_sort_modifier();
        }
        let Some(field) = Field::parse(field_name) else {
            // Unknown field → warning + freeform fallback. Re-attach
            // the colon and value so the user's typed expression is
            // preserved through evaluation.
            let value_str = self.consume_value_token_text();
            let combined = format!("{field_name}:{value_str}");
            self.warnings.push(combined.clone());
            return Ok(Expr::Text(combined));
        };
        // Comparison operators: `field:>value`, `field:<=value`, etc.
        if let Some(comp) = self.peek_and_eat_comparator() {
            let value = self.parse_value()?;
            return Ok(Expr::Compare { field, comp, value });
        }
        // Regex: `field:~pattern`.
        if matches!(self.peek(), Some(Token::Tilde)) {
            self.bump();
            let pattern = self.consume_value_token_text();
            return Ok(Expr::Field {
                field,
                kind: MatchKind::Regex(pattern),
            });
        }
        // Exact: `field:=value` or `field:"=value"`.
        if matches!(self.peek(), Some(Token::Eq)) {
            self.bump();
            let value = self.consume_value_token_text();
            return Ok(Expr::Field {
                field,
                kind: MatchKind::Exact(value),
            });
        }
        // Quoted with leading `=` inside the quotes — Calibre's
        // `tag:"=foo bar"` form.
        if let Some(Token::QuotedString(s)) = self.peek().cloned()
            && let Some(rest) = s.strip_prefix('=')
        {
            self.bump();
            return Ok(Expr::Field {
                field,
                kind: MatchKind::Exact(rest.to_string()),
            });
        }
        // Range: peek for `value..value`. The lexer surfaces the
        // dot-dot as its own token, so the shape is:
        //   bareword(low) DotDot bareword(high)
        if let Some(low_tok) = self.peek().cloned()
            && matches!(low_tok, Token::Bareword(_) | Token::QuotedString(_))
            && self.tokens.get(self.pos + 1) == Some(&Token::DotDot)
        {
            self.bump(); // low
            self.bump(); // ..
            let low_str = match &low_tok {
                Token::Bareword(s) | Token::QuotedString(s) => s.clone(),
                _ => unreachable!(),
            };
            let high_str = self.consume_value_token_text();
            return Ok(Expr::Range {
                field,
                low: parse_value(&low_str),
                high: parse_value(&high_str),
            });
        }
        // Default: substring or boolean-existence based on the
        // value. Date-shaped fields (`due:today`, `scheduled:thisweek`)
        // get a sense-correction: a bare date keyword on a date field
        // is treated as `Compare(Eq, ...)` — Calibre's convention.
        let value_str = self.consume_value_token_text();
        // v0.4.1 — `?prefix` on a text-shaped field becomes a fuzzy
        // (Levenshtein) match. The lexer keeps `?` inside the
        // bareword (no Token::Question), so we strip it here. Date /
        // numeric fields don't get fuzzy matching; the `?` stays as
        // a literal character of the value (which probably won't
        // match anything sensible — but that's correct behaviour
        // for "due:?today" rather than silently misinterpreting).
        if !field.is_date_shaped()
            && !field.is_numeric()
            && let Some(rest) = value_str.strip_prefix('?')
        {
            return Ok(Expr::Field {
                field,
                kind: MatchKind::Fuzzy(rest.to_string()),
            });
        }
        if field.is_date_shaped() {
            let v = parse_value(&value_str);
            if matches!(v, Value::Date(_) | Value::DateKeyword(_)) {
                return Ok(Expr::Compare {
                    field,
                    comp: Comparator::Eq,
                    value: v,
                });
            }
        }
        if field.is_numeric() {
            let v = parse_value(&value_str);
            if matches!(v, Value::Number(_)) {
                return Ok(Expr::Compare {
                    field,
                    comp: Comparator::Eq,
                    value: v,
                });
            }
        }
        let kind = match value_str.as_str() {
            "true" => MatchKind::HasAny,
            "false" => MatchKind::HasNone,
            _ => MatchKind::Substring(value_str),
        };
        Ok(Expr::Field { field, kind })
    }

    /// `sort:KEY` / `sort:-KEY` — read the next bareword and pull
    /// out the direction prefix and key name. Appends a SortSpec to
    /// self.sorts and returns `Expr::Pass` so the AST stays clean.
    fn parse_sort_modifier(&mut self) -> Result<Expr, ParseError> {
        let raw = self.consume_value_token_text();
        let (direction, key_name) = if let Some(rest) = raw.strip_prefix('-') {
            (SortDirection::Desc, rest)
        } else {
            (SortDirection::Asc, raw.as_str())
        };
        match SortKey::parse(key_name) {
            Some(key) => {
                self.sorts.push(SortSpec { key, direction });
                Ok(Expr::Pass)
            }
            None => {
                // Unknown sort key — warn, keep as freeform.
                let combined = format!("sort:{raw}");
                self.warnings.push(combined.clone());
                Ok(Expr::Text(combined))
            }
        }
    }

    /// `is:NAME` — read the next bareword and map it to a State.
    fn parse_state_predicate(&mut self) -> Result<Expr, ParseError> {
        // The right-hand side might be quoted (`is:"open"`) or bare.
        let raw = self.consume_value_token_text();
        match State::parse(&raw) {
            Some(state) => Ok(Expr::State(state)),
            None => {
                // Unknown state name — warn, keep as freeform.
                let combined = format!("is:{raw}");
                self.warnings.push(combined.clone());
                Ok(Expr::Text(combined))
            }
        }
    }

    fn peek_and_eat_comparator(&mut self) -> Option<Comparator> {
        let comp = match self.peek()? {
            Token::Eq => Comparator::Eq,
            Token::Ne => Comparator::Ne,
            Token::Lt => Comparator::Lt,
            Token::Le => Comparator::Le,
            Token::Gt => Comparator::Gt,
            Token::Ge => Comparator::Ge,
            _ => return None,
        };
        // `Eq` is ambiguous with the exact-match prefix; `parse_field_value`
        // handles `Eq` separately above so we only pick up the
        // comparison flavours here.
        if matches!(comp, Comparator::Eq) {
            return None;
        }
        self.bump();
        Some(comp)
    }

    fn parse_value(&mut self) -> Result<Value, ParseError> {
        let raw = self.consume_value_token_text();
        if raw.is_empty() {
            return Err(ParseError::BadComparison);
        }
        Ok(parse_value(&raw))
    }

    /// Consume the next token as a value's text representation. Bare
    /// words and quoted strings both apply; everything else returns
    /// an empty string (the caller decides if that's an error).
    fn consume_value_token_text(&mut self) -> String {
        match self.bump() {
            Some(Token::Bareword(s) | Token::QuotedString(s)) => s,
            _ => String::new(),
        }
    }
}

/// Best-effort string → Value conversion. Tries `NaiveDate`, then
/// the date-keyword set, then numeric, then falls back to text.
fn parse_value(s: &str) -> Value {
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Value::Date(d);
    }
    if let Some(k) = parse_date_keyword(s) {
        return Value::DateKeyword(k);
    }
    if let Ok(n) = s.parse::<i64>() {
        return Value::Number(n);
    }
    Value::Text(s.to_string())
}

fn parse_date_keyword(s: &str) -> Option<DateKeyword> {
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "today" => Some(DateKeyword::Today),
        "yesterday" => Some(DateKeyword::Yesterday),
        "tomorrow" => Some(DateKeyword::Tomorrow),
        "thisweek" => Some(DateKeyword::ThisWeek),
        "lastweek" => Some(DateKeyword::LastWeek),
        "nextweek" => Some(DateKeyword::NextWeek),
        "thismonth" => Some(DateKeyword::ThisMonth),
        "lastmonth" => Some(DateKeyword::LastMonth),
        "nextmonth" => Some(DateKeyword::NextMonth),
        "thisyear" => Some(DateKeyword::ThisYear),
        _ => {
            // `Ndaysago` / `Ndaysout` patterns.
            if let Some(n_str) = lower.strip_suffix("daysago")
                && let Ok(n) = n_str.parse::<u32>()
            {
                return Some(DateKeyword::DaysAgo(n));
            }
            if let Some(n_str) = lower.strip_suffix("daysout")
                && let Ok(n) = n_str.parse::<u32>()
            {
                return Some(DateKeyword::DaysOut(n));
            }
            None
        }
    }
}

impl Field {
    /// Date-shaped fields take their values as dates, not strings.
    /// Used by the parser to sense-correct `due:today` etc. into
    /// comparison expressions, and by the evaluator to know which
    /// branch to take.
    pub fn is_date_shaped(&self) -> bool {
        matches!(
            self,
            Self::Due
                | Self::Scheduled
                | Self::Defer
                | Self::Created
                | Self::Modified
                | Self::Completed
        )
    }

    /// Numeric fields take integer values.
    pub fn is_numeric(&self) -> bool {
        matches!(self, Self::Estimated)
    }

    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "tag" | "tags" => Some(Self::Tag),
            "area" => Some(Self::Area),
            "project" => Some(Self::Project),
            "title" => Some(Self::Title),
            "note" | "notes" => Some(Self::Note),
            "due" | "deadline" => Some(Self::Due),
            "scheduled" | "when" => Some(Self::Scheduled),
            "defer" | "defer_until" | "deferred" => Some(Self::Defer),
            "created" => Some(Self::Created),
            "modified" | "updated" => Some(Self::Modified),
            "completed" | "done" => Some(Self::Completed),
            "estimated" | "est" | "effort" => Some(Self::Estimated),
            "repeats" | "repeating" => Some(Self::Repeats),
            _ => None,
        }
    }
}

impl SortKey {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "due" | "deadline" => Some(Self::Due),
            "scheduled" | "when" => Some(Self::Scheduled),
            "defer" | "defer_until" | "deferred" => Some(Self::Defer),
            "created" => Some(Self::Created),
            "modified" | "updated" => Some(Self::Modified),
            "completed" | "done" => Some(Self::Completed),
            "estimated" | "est" | "effort" => Some(Self::Estimated),
            "title" => Some(Self::Title),
            "position" | "manual" => Some(Self::Position),
            _ => None,
        }
    }
}

impl State {
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "done" | "complete" | "completed" => Some(Self::Done),
            "overdue" => Some(Self::Overdue),
            "scheduled" => Some(Self::Scheduled),
            "deadline" => Some(Self::Deadline),
            "deferred" => Some(Self::Deferred),
            "repeating" => Some(Self::Repeating),
            "archived" => Some(Self::Archived),
            "logbook" => Some(Self::Logbook),
            "project" | "in_project" | "inproject" => Some(Self::InProject),
            "area" | "in_area" | "inarea" => Some(Self::InArea),
            "tagged" => Some(Self::Tagged),
            "queued" => Some(Self::Queued),
            "available" => Some(Self::Available),
            "blocked" => Some(Self::Blocked),
            // v0.4.1 — canonical-list mirrors. Each maps `is:NAME` to
            // the same membership query the corresponding sidebar list
            // uses (per spec §4.2).
            "today" => Some(Self::Today),
            "inbox" => Some(Self::Inbox),
            "upcoming" => Some(Self::Upcoming),
            "anytime" => Some(Self::Anytime),
            "someday" => Some(Self::Someday),
            _ => None,
        }
    }
}
