import re

with open("atrium-core/src/search/eval.rs", "r") as f:
    text = f.read()

# Replace imports
old_import = """use super::ast::{Comparator, DateKeyword, Expr, Field, MatchKind, State, Value};"""
new_import = """use vir_search::ast::{Comparator, Expr, MatchKind, Value};
use super::domain::{Field, State};"""
text = text.replace(old_import, new_import)

text = text.replace("use super::dates;", "use vir_search::dates;")
text = text.replace("use super::fold::fold;", "use vir_search::fold::fold;")
text = text.replace("use super::rank::fuzzy_threshold;", "")
text = text.replace("use super::rank::fuzzy_hit;", "")
text = text.replace("Expr::Pass => true,", "")
text = text.replace("Expr::Pass =>", "")

# The DateKeyword is handled by Value::Date. Atrium parser mapped keywords to Value::DateKeyword, vir-search maps everything directly to Value::Date in ast (actually ast::DateSpec).
# Wait, let's see how vir-search handles DateSpec vs DateKeyword.
