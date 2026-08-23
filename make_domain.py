import re

with open("atrium-search/src/ast.rs", "r") as f:
    ast = f.read()

# We only need the domain enums.
domain_enums = []
for enum_name in ["SortKey", "Field", "State"]:
    pattern = rf"(#\[derive[^\]]*\]\s*pub enum {enum_name} {{.*?}})"
    match = re.search(pattern, ast, re.DOTALL)
    if match:
        domain_enums.append(match.group(1))

with open("atrium-search/src/parse.rs", "r") as f:
    parse = f.read()

impls = []

# Field::parse
match = re.search(r"impl Field \{.*?pub fn parse\(s: &str\) -> Option<Self> \{(.*?)\}\n\}", parse, re.DOTALL)
if match:
    field_parse = match.group(1)
else:
    match = re.search(r"impl Field \{.*?fn parse\(s: &str\) -> Option<Self> \{(.*?)\}\n\}", parse, re.DOTALL)
    if match:
        field_parse = match.group(1)
    else:
        field_parse = """
        match s.to_ascii_lowercase().as_str() {
            "tag" | "tags" => Some(Self::Tag),
            "area" => Some(Self::Area),
            "project" => Some(Self::Project),
            "title" => Some(Self::Title),
            "note" => Some(Self::Note),
            "due" | "deadline" => Some(Self::Due),
            "scheduled" => Some(Self::Scheduled),
            "defer" | "defer_until" => Some(Self::Defer),
            "created" => Some(Self::Created),
            "modified" => Some(Self::Modified),
            "completed" => Some(Self::Completed),
            "estimated" | "est" => Some(Self::Estimated),
            "repeats" => Some(Self::Repeats),
            _ => None,
        }
"""

match = re.search(r"impl State \{.*?fn parse\(s: &str\) -> Option<Self> \{(.*?)\}\n\}", parse, re.DOTALL)
if match:
    state_parse = match.group(1)

match = re.search(r"impl SortKey \{.*?fn parse\(s: &str\) -> Option<Self> \{(.*?)\}\n\}", parse, re.DOTALL)
if match:
    sort_parse = match.group(1)


out = """use vir_search::ast::{FieldType, ParseField, ParseState, ParseSort};
use std::fmt;

"""
out += "\n\n".join(domain_enums)
out += "\n\n"

out += """
impl ParseField for Field {
    fn parse(name: &str) -> Option<Self> {
""" + field_parse + """
    }

    fn field_type(&self) -> FieldType {
        match self {
            Self::Estimated => FieldType::Int,
            Self::Due | Self::Scheduled | Self::Defer | Self::Created | Self::Modified | Self::Completed => FieldType::Date,
            _ => FieldType::String,
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tag => write!(f, "tag"),
            Self::Area => write!(f, "area"),
            Self::Project => write!(f, "project"),
            Self::Title => write!(f, "title"),
            Self::Note => write!(f, "note"),
            Self::Due => write!(f, "due"),
            Self::Scheduled => write!(f, "scheduled"),
            Self::Defer => write!(f, "defer"),
            Self::Created => write!(f, "created"),
            Self::Modified => write!(f, "modified"),
            Self::Completed => write!(f, "completed"),
            Self::Estimated => write!(f, "estimated"),
            Self::Repeats => write!(f, "repeats"),
        }
    }
}

impl ParseState for State {
    fn parse(name: &str) -> Option<Self> {
""" + state_parse + """
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Done => write!(f, "done"),
            Self::Overdue => write!(f, "overdue"),
            Self::Scheduled => write!(f, "scheduled"),
            Self::Deadline => write!(f, "deadline"),
            Self::Deferred => write!(f, "deferred"),
            Self::Repeating => write!(f, "repeating"),
            Self::Archived => write!(f, "archived"),
            Self::Logbook => write!(f, "logbook"),
            Self::InProject => write!(f, "project"),
            Self::InArea => write!(f, "area"),
            Self::Tagged => write!(f, "tagged"),
            Self::Queued => write!(f, "queued"),
            Self::Available => write!(f, "available"),
            Self::Blocked => write!(f, "blocked"),
            Self::Today => write!(f, "today"),
            Self::Inbox => write!(f, "inbox"),
            Self::Upcoming => write!(f, "upcoming"),
            Self::Anytime => write!(f, "anytime"),
            Self::Someday => write!(f, "someday"),
        }
    }
}

impl ParseSort for SortKey {
    fn parse(name: &str) -> Option<Self> {
""" + sort_parse + """
    }
}

impl fmt::Display for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Due => write!(f, "due"),
            Self::Scheduled => write!(f, "scheduled"),
            Self::Defer => write!(f, "defer"),
            Self::Created => write!(f, "created"),
            Self::Modified => write!(f, "modified"),
            Self::Completed => write!(f, "completed"),
            Self::Estimated => write!(f, "estimated"),
            Self::Title => write!(f, "title"),
            Self::Position => write!(f, "position"),
        }
    }
}
"""

with open("atrium-core/src/search/domain.rs", "w") as f:
    f.write(out)

