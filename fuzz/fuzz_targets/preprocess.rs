#![no_main]
//! Preprocess fuzz target (task 04-19).
//!
//! Exercises the full preprocessor pipeline — `Session::new` →
//! `SourceMap::add_file` → `Preprocessor::run` — against libFuzzer
//! mutated inputs. The contract is simple: for **every** input byte
//! sequence the preprocessor must either succeed, emit diagnostics, or
//! return a truncated pp-token stream. It must never panic, abort, or
//! run away long enough to hit libFuzzer's `-timeout` gate.
//!
//! Notes on input handling:
//!
//! * `SourceMap::add_file` takes `Arc<str>`, so non-UTF-8 bytes are
//!   folded through `String::from_utf8_lossy`. This keeps every
//!   mutation reachable by the preprocessor (rather than dropped at
//!   the door), at the cost of replacing invalid sequences with
//!   U+FFFD — acceptable since the fuzzer's job is the preprocessor,
//!   not UTF-8 validation.
//! * A hard input-size cap (`MAX_INPUT`) mirrors the lex target. The
//!   cargo alias `fuzz-preprocess` also passes `-max_len=131072` so
//!   libFuzzer itself won't produce larger inputs; the in-target
//!   check is defence-in-depth for direct-invocation reproduction.
//! * No recursion / token-budget cap is applied: pathological inputs
//!   that blow the stack or hang the expander **are** the bugs we
//!   want the fuzzer to surface (see the task's acceptance bullet
//!   "stack overflow on recursive macro is caught within seconds").

use std::path::PathBuf;
use std::sync::Arc;

use libfuzzer_sys::fuzz_target;

use rcc_preprocess::Preprocessor;
use rcc_session::{Options, Session};

/// Per-input byte cap (128 KiB, matching the `fuzz-preprocess` alias's
/// `-max_len`). Inputs above this are discarded so a stray direct
/// invocation can't drown the process in a multi-MiB blob.
const MAX_INPUT: usize = 128 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let src: Arc<str> = Arc::from(String::from_utf8_lossy(data).into_owned());

    let mut session = Session::new(Options::default());
    let file = {
        let mut sm = match session.source_map.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        sm.add_file(PathBuf::from("<fuzz>"), src)
    };
    let _ = Preprocessor::new(&mut session).run(file);
});
