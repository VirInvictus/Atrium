pub mod domain;
pub mod eval;
pub mod sql_translate;

pub use domain::{Field, SortKey, State};
pub use eval::{EvalContext, evaluate};
pub use sql_translate::try_translate;

pub type Expr = vir_search::ast::Expr<Field, State>;
pub type SortSpec = vir_search::ast::SortSpec<SortKey>;

pub type ParseResult = vir_search::parse::ParseResult<Field, State, SortKey>;

pub fn parse(query: &str) -> ParseResult {
    vir_search::parse::parse(query)
}

pub fn collect_text_terms(expr: &Expr) -> Vec<String> {
    vir_search::rank::collect_text_terms(expr)
}

pub fn blend_relevance(bm25: f64, days: i64, half_life: f64) -> f64 {
    vir_search::rank::blend_relevance(bm25, days, half_life)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}
