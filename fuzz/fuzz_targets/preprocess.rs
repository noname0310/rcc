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
//! * The preprocessor has an include-depth cap so recursive virtual
//!   include trees terminate before the process stack overflows. Other
//!   pathological macro-expansion inputs are still expected to surface
//!   as crashes or timeouts.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use libfuzzer_sys::fuzz_target;

use rcc_errors::{CaptureEmitter, Handler};
use rcc_preprocess::Preprocessor;
use rcc_session::{Options, Session};

/// Segment separator for multi-file fuzz inputs.
///
/// Segment 0 is the root translation unit. Segments 1..N are installed
/// as virtual headers using [`VIRTUAL_HEADER_NAMES`]. This keeps the
/// target deterministic and avoids per-input disk I/O while still
/// fuzzing C99 `#include` resolution, include guards, and `#pragma once`.
const VIRTUAL_FILE_SEPARATOR: &[u8] = b"\n/*__RCC_FUZZ_VIRTUAL_FILE__*/\n";

/// Fixed virtual header names addressable from fuzzed `#include` lines.
const VIRTUAL_HEADER_NAMES: &[&str] = &[
    "test.h",
    "include1.h",
    "include2.h",
    "include3.h",
    "include4.h",
    "test/pragma-once.c",
    "fuzz0.h",
    "fuzz1.h",
];

/// Virtual root used as both the root file's directory and a system include path.
const VIRTUAL_ROOT: &str = "__rcc_fuzz_vfs__";

/// Per-input byte cap (128 KiB, matching the `fuzz-preprocess` alias's
/// `-max_len`). Inputs above this are discarded so a stray direct
/// invocation can't drown the process in a multi-MiB blob.
const MAX_INPUT: usize = 128 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let segments = split_segments(data, VIRTUAL_FILE_SEPARATOR);
    let Some((main_bytes, header_bytes)) = segments.split_first() else {
        return;
    };
    let src: Arc<str> = Arc::from(String::from_utf8_lossy(main_bytes).into_owned());

    let root = PathBuf::from(VIRTUAL_ROOT);
    let main_path = root.join("main.c");
    let handler = Handler::with_emitter(Box::new(CaptureEmitter::new()));
    let mut session = Session::with_handler(
        Options { include_paths: vec![root.clone()], ..Options::default() },
        handler,
    );
    for (name, bytes) in VIRTUAL_HEADER_NAMES.iter().zip(header_bytes.iter().copied()) {
        session.add_virtual_file(root.join(Path::new(name)), lossless_fuzz_text(bytes));
    }
    let file = session
        .source_map
        .write()
        .map(|mut sm| sm.add_file(main_path, src))
        .ok();
    let Some(file) = file else {
        return;
    };
    let _ = Preprocessor::new(&mut session).run(file);
});

fn split_segments<'a>(mut data: &'a [u8], separator: &[u8]) -> Vec<&'a [u8]> {
    if separator.is_empty() {
        return vec![data];
    }

    let mut segments = Vec::new();
    while let Some(pos) = find_bytes(data, separator) {
        let (head, tail) = data.split_at(pos);
        segments.push(head);
        data = &tail[separator.len()..];
    }
    segments.push(data);
    segments
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn lossless_fuzz_text(bytes: &[u8]) -> Arc<str> {
    Arc::from(String::from_utf8_lossy(bytes).into_owned())
}
