import re

with open("atrium-core/src/search/sql_translate.rs", "r") as f:
    text = f.read()

text = text.replace("use crate::ast::", "use vir_search::ast::")
text = text.replace("use crate::dates::value_to_range;", "")
text = text.replace("value_to_range(", "vir_search::dates::resolve_range(")
text = text.replace("atrium_core::SqlBindValue", "crate::db::SqlBindValue")
text = text.replace("atrium_core::domain::", "crate::domain::")
text = text.replace("vir_search::ast::{Comparator, Expr, Field, MatchKind, State, Value};", "vir_search::ast::{Comparator, Expr, MatchKind, Value};\nuse crate::search::domain::{Field, State};")
text = text.replace("use super::ast::{Comparator, Expr, Field, MatchKind, State, Value};", "use vir_search::ast::{Comparator, Expr, MatchKind, Value};\nuse crate::search::domain::{Field, State};")

with open("atrium-core/src/search/sql_translate.rs", "w") as f:
    f.write(text)

with open("atrium-core/src/search/eval.rs", "r") as f:
    text = f.read()
text = text.replace("atrium_core::domain::", "crate::domain::")
text = text.replace("use super::dates::{compare_date, value_to_range};", "use vir_search::dates::{matches as compare_date, resolve_range as value_to_range};")
text = text.replace("super::domain", "crate::search::domain")

with open("atrium-core/src/search/eval.rs", "w") as f:
    f.write(text)

