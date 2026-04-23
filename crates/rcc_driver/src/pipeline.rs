//! Orchestration of the compiler pipeline: source -> preprocess -> parse ->
//! lower -> typeck -> cfg-build -> cfg-transform -> codegen.

use std::path::Path;

use rcc_cfg::build_bodies;
use rcc_codegen_llvm::{codegen, CodegenError};
use rcc_hir::TyCtxt;
use rcc_hir_lower::lower;
use rcc_preprocess::preprocess;
use rcc_session::{EmitKind, Session};
use rcc_typeck::check;

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

    // 2. Preprocess.
    let pp_tokens = preprocess(session, file);
    if session.opts.emit.contains(&EmitKind::Pp) {
        eprintln!("-- emit=pp: {} pp-tokens", pp_tokens.len());
    }

    // 3. Parse.
    let ast = match rcc_parse::parse(session, pp_tokens) {
        Some(ast) => ast,
        None => return Ok(()), // Errors already reported.
    };
    if session.opts.emit.contains(&EmitKind::Ast) {
        eprintln!("-- emit=ast: {} decls", ast.decls.len());
    }

    // 4. Lower to HIR.
    let mut tcx = TyCtxt::new();
    let mut hir = lower(&ast, &mut tcx, session);

    // 5. Type check.
    check(session, &mut tcx, &mut hir);
    if session.handler.has_errors() {
        return Ok(());
    }

    // 6. Build CFG.
    let bodies = build_bodies(session, &tcx, &hir);

    // 7. Codegen.
    match codegen(session, &tcx, &hir, &bodies) {
        Ok(_art) => Ok(()),
        Err(CodegenError::BackendDisabled) => {
            // Skeleton build; not an error, just a notice for now.
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}
