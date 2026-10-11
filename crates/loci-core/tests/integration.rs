//! Integration tests over the real `Collection` on a file-backed temp directory (recall
//! `tests/unit/integration/*.test.ts`). The fixture's day numbers are defined in UTC, so every
//! harness uses a zero time-zone offset and a fixed clock at the fixture's "now".

mod integration {
    pub mod helpers;

    mod backlog_browse;
    mod import;
    mod perf;
    mod rpc;
    mod stats_export;
    mod study;
}
