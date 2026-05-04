#![no_main]
//! Parse fuzz target (task 12-03).
//!
//! Pipeline: bytes -> SourceMap -> preprocess -> parser phase 7 -> C99 AST.
//! The fuzzer contract is no panic / abort / runaway recovery loop for any
//! byte sequence. Invalid C is expected; diagnostics are ordinary output.

use std::path::PathBuf;
use std::sync::Arc;

use libfuzzer_sys::fuzz_target;

use rcc_errors::{CaptureEmitter, Handler};
use rcc_preprocess::preprocess;
use rcc_session::{Options, Session};

const MAX_INPUT: usize = 128 * 1024;
const MAX_PP_TOKENS: usize = 64 * 1024;
const MAX_DIAGNOSTICS: usize = 2048;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() || data.len() > MAX_INPUT {
        return;
    }

    let src: Arc<str> = Arc::from(String::from_utf8_lossy(data).into_owned());
    let cap = CaptureEmitter::new();
    let handler = Handler::with_emitter(Box::new(cap.clone()));
    let mut session = Session::with_handler(Options::default(), handler);
    let file = session
        .source_map
        .write()
        .map(|mut sm| sm.add_file(PathBuf::from("__rcc_parse_fuzz__.c"), src))
        .ok();
    let Some(file) = file else {
        return;
    };

    let pp_tokens = preprocess(&mut session, file);
    if pp_tokens.len() > MAX_PP_TOKENS {
        return;
    }
    let token_ratio_limit = data.len().saturating_mul(64).saturating_add(1024);
    if pp_tokens.len() > token_ratio_limit {
        return;
    }
    if cap.diagnostics().len() > MAX_DIAGNOSTICS {
        return;
    }

    let _ = rcc_parse::parse(&mut session, pp_tokens);
});
