//! Orchestration of the compiler pipeline: source -> preprocess -> parse ->
//! lower -> typeck -> cfg-build -> cfg-transform -> codegen.

use std::path::Path;

use rcc_cfg::{build_bodies, pretty::dump_body};
use rcc_codegen_llvm::{codegen, CodegenError};
use rcc_hir::TyCtxt;
use rcc_hir_lower::lower;
use rcc_lexer::{PpToken, PpTokenKind};
use rcc_preprocess::preprocess;
use rcc_session::{EmitKind, Session};
use rcc_typeck::{check, verify_typed_hir};

/// Compile a single file end-to-end. Errors are written to the session's
/// diagnostic handler; this function only returns `Err` for unrecoverable
/// I/O or backend failures.
pub fn compile(session: &mut Session, input: &Path) -> Result<(), String> {
    // 1. Load.
    let file = session
        .source_map
        .write()
        .unwrap()
        .load_file(input)
        .map_err(|e| format!("cannot read {}: {e}", input.display()))?;

    // 1b. `--emit=tokens`: run the raw lexer against the loaded source
    //     and print the pretty-printed pp-token stream to stdout. This
    //     happens BEFORE preprocessing so it reflects phase-03 output
    //     exclusively; macro expansion / directive handling belong to
    //     a later `--emit` stage.
    if session.opts.emit.contains(&EmitKind::Tokens) {
        let sm = session.source_map.read().unwrap();
        let src = sm.file(file).src.clone();
        let out = rcc_lexer::pretty::format_tokens(&src, &sm, file);
        print!("{out}");
        if !has_later_emit_than_tokens(&session.opts.emit) {
            return Ok(());
        }
    }

    // 2. Preprocess.
    let pp_tokens = preprocess(session, file);
    if session.opts.emit.contains(&EmitKind::Pp) {
        emit_preprocessed(session, &pp_tokens);
        // When `--emit=pp` is requested and no later stage is asked
        // for, the driver stops here: it would be surprising for
        // `rcc --emit=pp foo.c` to then try to parse / typecheck /
        // codegen a file whose preprocessing output was the entire
        // point. Downstream crates (parse / typeck / codegen) also
        // don't tolerate the full GCC / Clang extension surface
        // that a preprocessor-only run may well exercise, so running
        // them past a `--emit=pp` request muddies the exit code.
        //
        // Stopping is conditional on no *later* stage being set;
        // a user who asks for `--emit=pp,obj` still gets the full
        // pipeline. Task 04-18 (chibicc preprocessor tests) relies
        // on this short-circuit so a conformance adapter can run
        // preprocess-only checks.
        if !has_later_emit_than_pp(&session.opts.emit) {
            return Ok(());
        }
    }

    // 3. Parse.
    let ast = match rcc_parse::parse(session, pp_tokens) {
        Some(ast) => ast,
        None => return Ok(()), // Errors already reported.
    };
    if session.opts.emit.contains(&EmitKind::Ast) {
        eprintln!("-- emit=ast: {} decls", ast.decls.len());
        if !has_later_emit_than_ast(&session.opts.emit) {
            return Ok(());
        }
    }

    // 4. Lower to HIR.
    let mut tcx = TyCtxt::new();
    let mut hir = lower(&ast, &mut tcx, session);

    // 5. Type check.
    check(session, &mut tcx, &mut hir);
    if session.handler.has_errors() {
        return Ok(());
    }
    verify_typed_hir(session, &tcx, &hir);
    if session.handler.has_errors() {
        return Ok(());
    }
    if session.opts.emit.contains(&EmitKind::Hir) && !has_later_emit_than_hir(&session.opts.emit) {
        return Ok(());
    }

    // 6. Build CFG.
    let bodies = build_bodies(session, &tcx, &hir);
    if session.opts.emit.contains(&EmitKind::Mir) {
        emit_mir(&tcx, &bodies);
        if !backend_required(&session.opts.emit) {
            return Ok(());
        }
    }

    // 7. Codegen.
    match codegen(session, &tcx, &hir, &bodies) {
        Ok(_art) => Ok(()),
        Err(CodegenError::BackendDisabled) => Err(CodegenError::BackendDisabled.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

fn emit_mir(tcx: &TyCtxt, bodies: &rcc_data_structures::FxHashMap<rcc_hir::DefId, rcc_cfg::Body>) {
    let mut ids: Vec<_> = bodies.keys().copied().collect();
    ids.sort_by_key(|id| id.0);
    for (idx, id) in ids.iter().enumerate() {
        if idx > 0 {
            println!();
        }
        if let Some(body) = bodies.get(id) {
            print!("{}", dump_body(body, tcx));
        }
    }
}

/// Write a human-readable rendering of the preprocessed pp-token
/// stream to stdout, one token per space, newlines inserted between
/// tokens whose spans cross a source-line boundary. This is not an
/// exact `cc -E` reproduction — different hosts disagree on spacing
/// anyway — but it is enough for eyeballing and for the conformance
/// runner's "preprocess mode" to tell a run that produced output
/// apart from one that didn't.
///
/// The function is deliberately tolerant: synthetic tokens (with
/// virtual-file spans) contribute their raw spelling; missing
/// files are silently skipped rather than panicking, because the
/// token stream is already the preprocessor's best effort and
/// printing is a best-effort downstream concern.
fn emit_preprocessed(session: &Session, tokens: &[PpToken]) {
    let sm = session.source_map.read().unwrap();
    let mut prev_line: Option<u32> = None;
    let mut prev_file: Option<rcc_span::FileId> = None;
    let mut buf = String::new();
    for tok in tokens {
        if matches!(tok.kind, PpTokenKind::Whitespace | PpTokenKind::Newline | PpTokenKind::Eof) {
            continue;
        }
        let file = sm.file(tok.span.file);
        let line = sm.lookup_line_col(tok.span.file, tok.span.lo).line;
        if let (Some(pf), Some(pl)) = (prev_file, prev_line) {
            if pf != tok.span.file || line != pl {
                buf.push('\n');
            } else {
                buf.push(' ');
            }
        }
        let lo = tok.span.lo.0 as usize;
        let hi = tok.span.hi.0 as usize;
        if hi <= file.src.len() {
            buf.push_str(&file.src[lo..hi]);
        }
        prev_line = Some(line);
        prev_file = Some(tok.span.file);
    }
    if !buf.is_empty() {
        buf.push('\n');
    }
    print!("{buf}");
}

/// Whether `emit` contains any stage that runs after preprocessing.
/// Used by the `--emit=pp` short-circuit: if the user only asked for
/// `Tokens` and / or `Pp`, we can stop the pipeline after phase 4
/// without losing information.
fn has_later_emit_than_pp(emit: &[EmitKind]) -> bool {
    emit.iter().any(|k| {
        matches!(
            k,
            EmitKind::Ast
                | EmitKind::Hir
                | EmitKind::Mir
                | EmitKind::LlvmIr
                | EmitKind::Asm
                | EmitKind::Obj
        )
    })
}

fn has_later_emit_than_tokens(emit: &[EmitKind]) -> bool {
    emit.iter().any(|k| {
        matches!(
            k,
            EmitKind::Pp
                | EmitKind::Ast
                | EmitKind::Hir
                | EmitKind::Mir
                | EmitKind::LlvmIr
                | EmitKind::Asm
                | EmitKind::Obj
        )
    })
}

fn has_later_emit_than_ast(emit: &[EmitKind]) -> bool {
    emit.iter().any(|k| {
        matches!(
            k,
            EmitKind::Hir | EmitKind::Mir | EmitKind::LlvmIr | EmitKind::Asm | EmitKind::Obj
        )
    })
}

fn has_later_emit_than_hir(emit: &[EmitKind]) -> bool {
    emit.iter()
        .any(|k| matches!(k, EmitKind::Mir | EmitKind::LlvmIr | EmitKind::Asm | EmitKind::Obj))
}

fn backend_required(emit: &[EmitKind]) -> bool {
    emit.is_empty()
        || emit.iter().any(|k| matches!(k, EmitKind::LlvmIr | EmitKind::Asm | EmitKind::Obj))
}
