//! Anki search syntax (docs/SEARCH.md): parser → AST, AST → SQL over
//! the collection schema with an exact post-filter for the operators SQLite cannot express, a
//! runner, and a one-sentence English description of a query.
pub mod ast;
pub mod describe;
pub mod parse;
pub mod sql;
#[cfg(test)]
mod tests;

pub use describe::describe_query;
pub use parse::{
    glob_to_like, glob_to_regex_source, has_wildcards, parse_search, unescape_quotes, unescape_text, CompareOp, FieldMode,
    IdTarget, PropKind, PropValue, SearchError, SearchNode, StateKind,
};
pub use sql::{
    build_search_select, compile_search, load_field_index, register_sql_functions, retrievability_sql, run_search,
    run_search_ast, search_to_sql, user_regex, CompiledSearch, FieldInfo, PostFilterRow, SearchContext, SearchMode,
    FSRS6_DEFAULT_DECAY,
};
