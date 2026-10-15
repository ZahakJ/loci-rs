//! The search AST and its error type: what
//! `parse::parse_search` produces and `sql`/`describe` consume.

use crate::error::LociError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CompareOp {
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = "<=")]
    Le,
    #[serde(rename = ">=")]
    Ge,
    #[serde(rename = "=")]
    Eq,
    #[serde(rename = "!=")]
    Ne,
}

impl CompareOp {
    pub fn as_str(self) -> &'static str {
        match self {
            CompareOp::Lt => "<",
            CompareOp::Gt => ">",
            CompareOp::Le => "<=",
            CompareOp::Ge => ">=",
            CompareOp::Eq => "=",
            CompareOp::Ne => "!=",
        }
    }
    pub fn parse(s: &str) -> Option<CompareOp> {
        Some(match s {
            "<" => CompareOp::Lt,
            ">" => CompareOp::Gt,
            "<=" => CompareOp::Le,
            ">=" => CompareOp::Ge,
            "=" => CompareOp::Eq,
            "!=" => CompareOp::Ne,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StateKind {
    Due,
    New,
    Learn,
    Review,
    Suspended,
    Buried,
    BuriedSibling,
    BuriedManually,
}

impl StateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StateKind::Due => "due",
            StateKind::New => "new",
            StateKind::Learn => "learn",
            StateKind::Review => "review",
            StateKind::Suspended => "suspended",
            StateKind::Buried => "buried",
            StateKind::BuriedSibling => "buried-sibling",
            StateKind::BuriedManually => "buried-manually",
        }
    }
    pub fn parse(s: &str) -> Option<StateKind> {
        Some(match s {
            "due" => StateKind::Due,
            "new" => StateKind::New,
            "learn" => StateKind::Learn,
            "review" => StateKind::Review,
            "suspended" => StateKind::Suspended,
            "buried" => StateKind::Buried,
            "buried-sibling" => StateKind::BuriedSibling,
            "buried-manually" => StateKind::BuriedManually,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PropKind {
    Ivl,
    Due,
    Reps,
    Lapses,
    Ease,
    Pos,
    R,
    S,
    D,
    Rated,
    Resched,
    Cdn,
    Cds,
}

impl PropKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PropKind::Ivl => "ivl",
            PropKind::Due => "due",
            PropKind::Reps => "reps",
            PropKind::Lapses => "lapses",
            PropKind::Ease => "ease",
            PropKind::Pos => "pos",
            PropKind::R => "r",
            PropKind::S => "s",
            PropKind::D => "d",
            PropKind::Rated => "rated",
            PropKind::Resched => "resched",
            PropKind::Cdn => "cdn",
            PropKind::Cds => "cds",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PropValue {
    Num(f64),
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldMode {
    Glob,
    Regex,
    Nc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IdTarget {
    Nid,
    Cid,
    Mid,
    Did,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum SearchNode {
    All,
    And {
        children: Vec<SearchNode>,
    },
    Or {
        children: Vec<SearchNode>,
    },
    Not {
        child: Box<SearchNode>,
    },
    /// unqualified text; `text` is an escaped glob
    Text {
        text: String,
    },
    /// `field:value`; `field` and `value` are escaped globs (value raw regex for mode Regex)
    Field {
        field: String,
        value: String,
        mode: FieldMode,
    },
    Deck {
        name: String,
    },
    DeckCurrent,
    Notetype {
        name: String,
    },
    CardOrd {
        ord: i64,
    },
    CardName {
        name: String,
    },
    Tag {
        tag: String,
    },
    TagNone,
    TagRegex {
        pattern: String,
    },
    State {
        state: StateKind,
    },
    Flag {
        flag: i64,
    },
    /// `key` for cdn/cds; numeric `value` except cds (text)
    Prop {
        prop: PropKind,
        op: CompareOp,
        value: PropValue,
        #[serde(skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    Rated {
        days: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        ease: Option<i64>,
    },
    Resched {
        days: i64,
    },
    Introduced {
        days: i64,
    },
    Added {
        days: i64,
    },
    Edited {
        days: i64,
    },
    Ids {
        target: IdTarget,
        ids: Vec<i64>,
    },
    Dupe {
        ntid: i64,
        text: String,
    },
    HasCd {
        key: String,
    },
    Preset {
        name: String,
    },
    Regex {
        pattern: String,
    },
    Nc {
        text: String,
    },
    Word {
        text: String,
    },
    Sc {
        text: String,
    },
}

/// A syntax error with character offsets into the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchError {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

impl SearchError {
    pub fn new(message: impl Into<String>, start: usize, end: usize) -> SearchError {
        SearchError { message: message.into(), start, end }
    }
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SearchError {}

impl From<SearchError> for LociError {
    fn from(e: SearchError) -> Self {
        LociError::invalid(e.message).with_detail(format!("{}:{}", e.start, e.end))
    }
}
