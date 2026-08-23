import re

with open("atrium-core/src/search/eval.rs", "r") as f:
    text = f.read()

text = text.replace("use vir_search::dates::{matches as compare_date, resolve_range as value_to_range};", "")

adapter = """
fn value_to_range(val: &vir_search::ast::Value, today: chrono::NaiveDate) -> (chrono::NaiveDate, chrono::NaiveDate) {
    if let vir_search::ast::Value::Date(spec) = val {
        let (lo_epoch, hi_epoch) = vir_search::dates::resolve_range(spec, today);
        let lo = chrono::DateTime::from_timestamp(lo_epoch, 0).unwrap().naive_utc().date();
        let hi = chrono::DateTime::from_timestamp(hi_epoch, 0).unwrap().naive_utc().date();
        (lo, hi)
    } else {
        unreachable!()
    }
}

fn compare_date(
    d: chrono::NaiveDate,
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
    comp: &vir_search::ast::Comparator,
) -> bool {
    let d = d.and_time(chrono::NaiveTime::MIN).and_utc().timestamp();
    let start = start.and_time(chrono::NaiveTime::MIN).and_utc().timestamp();
    let end = end.and_time(chrono::NaiveTime::MIN).and_utc().timestamp();
    vir_search::dates::matches(*comp, d, start, end)
}
"""

text = text + adapter

with open("atrium-core/src/search/eval.rs", "w") as f:
    f.write(text)

