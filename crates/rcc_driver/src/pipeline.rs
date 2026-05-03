//! Orchestration of the compiler pipeline: source -> preprocess -> parse ->
//! lower -> typeck -> cfg-build -> cfg-transform -> codegen.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

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
    let mut stage_outputs = Vec::new();

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
        stage_outputs.push(StageOutput::text(EmitKind::Tokens, out));
        if !has_later_emit_than_tokens(&session.opts.emit) {
            return flush_stage_outputs(session, input, &stage_outputs);
        }
    }

    // 2. Preprocess.
    let pp_tokens = preprocess(session, file);
    if session.opts.emit.contains(&EmitKind::Pp) {
        stage_outputs
            .push(StageOutput::text(EmitKind::Pp, format_preprocessed(session, &pp_tokens)));
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
            return flush_stage_outputs(session, input, &stage_outputs);
        }
    }

    // 3. Parse.
    let ast = match rcc_parse::parse(session, pp_tokens) {
        Some(ast) => ast,
        None => return Ok(()), // Errors already reported.
    };
    if session.opts.emit.contains(&EmitKind::Ast) {
        stage_outputs
            .push(StageOutput::text(EmitKind::Ast, rcc_ast::pretty::dump_translation_unit(&ast)));
        if !has_later_emit_than_ast(&session.opts.emit) {
            return flush_stage_outputs(session, input, &stage_outputs);
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
    if session.opts.emit.contains(&EmitKind::Hir) {
        stage_outputs.push(StageOutput::text(EmitKind::Hir, rcc_hir::pretty::dump_crate(&hir)));
        if !has_later_emit_than_hir(&session.opts.emit) {
            return flush_stage_outputs(session, input, &stage_outputs);
        }
    }

    // 6. Build CFG.
    let bodies = build_bodies(session, &tcx, &hir);
    if session.opts.emit.contains(&EmitKind::Mir) {
        stage_outputs.push(StageOutput::text(EmitKind::Mir, format_mir(&tcx, &bodies)));
        if !backend_required(&session.opts.emit) {
            return flush_stage_outputs(session, input, &stage_outputs);
        }
    }

    // 7. Codegen.
    match codegen(session, &tcx, &hir, &bodies) {
        Ok(art) => {
            if session.opts.emit.contains(&EmitKind::LlvmIr) {
                stage_outputs.push(StageOutput::text(EmitKind::LlvmIr, art.ir_text));
            }
            if session.opts.emit.contains(&EmitKind::Asm) {
                let assembly = art
                    .assembly_text
                    .ok_or_else(|| "LLVM backend did not return assembly output".to_string())?;
                stage_outputs.push(StageOutput::text(EmitKind::Asm, assembly));
            }
            if session.opts.emit.is_empty() {
                let object = art
                    .object_bytes
                    .ok_or_else(|| "LLVM backend did not return object output".to_string())?;
                let exe = output_executable_path(session, input);
                let obj = TempObject::new(input);
                fs::write(obj.path(), object)
                    .map_err(|e| format!("cannot write {}: {e}", obj.path().display()))?;
                return link(obj.path(), &exe);
            }
            if session.opts.emit.contains(&EmitKind::Obj) {
                let object = art
                    .object_bytes
                    .ok_or_else(|| "LLVM backend did not return object output".to_string())?;
                stage_outputs.push(StageOutput::bytes(EmitKind::Obj, object));
            }
            flush_stage_outputs(session, input, &stage_outputs)
        }
        Err(CodegenError::BackendDisabled) => Err(CodegenError::BackendDisabled.to_string()),
        Err(e) => Err(e.to_string()),
    }
}

struct StageOutput {
    kind: EmitKind,
    bytes: Vec<u8>,
}

impl StageOutput {
    fn text(kind: EmitKind, text: String) -> Self {
        Self { kind, bytes: text.into_bytes() }
    }

    fn bytes(kind: EmitKind, bytes: Vec<u8>) -> Self {
        Self { kind, bytes }
    }
}

fn flush_stage_outputs(
    session: &Session,
    input: &Path,
    outputs: &[StageOutput],
) -> Result<(), String> {
    if outputs.is_empty() {
        return Ok(());
    }

    if outputs.len() == 1 && session.opts.emit.len() == 1 {
        if let Some(path) = &session.opts.output {
            fs::write(path, &outputs[0].bytes)
                .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        } else {
            io::stdout()
                .write_all(&outputs[0].bytes)
                .map_err(|e| format!("cannot write stdout: {e}"))?;
        }
        return Ok(());
    }

    let base = session.opts.output.as_deref().unwrap_or(input);
    for output in outputs {
        let path = stage_output_path(base, output.kind);
        fs::write(&path, &output.bytes)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(())
}

/// Link one native object file into an executable using the host C compiler.
///
/// This deliberately goes through `cc` instead of invoking `ld` directly so
/// libc and CRT startup objects stay the host toolchain's responsibility.
pub fn link(obj: &Path, output: &Path) -> Result<(), String> {
    let linker = find_host_cc()?;
    link_with_linker(&linker, obj, output)
}

/// Link with an explicit linker path. Public for driver tests and later tool
/// discovery work; ordinary users should call [`link`].
pub fn link_with_linker(linker: &Path, obj: &Path, output: &Path) -> Result<(), String> {
    let command = LinkCommand::new(linker.to_path_buf(), obj, output);
    let result = Command::new(&command.program)
        .args(&command.args)
        .output()
        .map_err(|e| format!("failed to run linker `{}`: {e}", command.render()))?;

    if result.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&result.stderr);
    Err(format!(
        "linker failed with status {}\ncommand: {}\nstderr:\n{}",
        result.status,
        command.render(),
        stderr.trim_end()
    ))
}

/// A host linker command before it is spawned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkCommand {
    program: PathBuf,
    args: Vec<OsString>,
}

impl LinkCommand {
    /// Build `cc <obj> -o <output>`.
    #[must_use]
    pub fn new(program: PathBuf, obj: &Path, output: &Path) -> Self {
        Self {
            program,
            args: vec![
                obj.as_os_str().to_owned(),
                OsString::from("-o"),
                output.as_os_str().to_owned(),
            ],
        }
    }

    /// Render the command line for diagnostics.
    #[must_use]
    pub fn render(&self) -> String {
        std::iter::once(quote_arg(self.program.as_os_str()))
            .chain(self.args.iter().map(|arg| quote_arg(arg.as_os_str())))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn find_host_cc() -> Result<PathBuf, String> {
    find_program_on_path("cc").ok_or_else(|| "host linker `cc` was not found on PATH".to_string())
}

fn find_program_on_path(program: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        for name in executable_names(program) {
            let candidate = dir.join(&name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn executable_names(program: &str) -> Vec<OsString> {
    #[cfg(windows)]
    {
        let has_ext = Path::new(program).extension().is_some();
        if has_ext {
            return vec![OsString::from(program)];
        }
        let pathext =
            env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
        let mut names = vec![OsString::from(program)];
        for ext in pathext.to_string_lossy().split(';').filter(|ext| !ext.is_empty()) {
            names.push(OsString::from(format!("{program}{ext}")));
        }
        names
    }
    #[cfg(not(windows))]
    {
        vec![OsString::from(program)]
    }
}

fn quote_arg(arg: &OsStr) -> String {
    let raw = arg.to_string_lossy();
    if raw.is_empty() || raw.chars().any(char::is_whitespace) {
        format!("\"{}\"", raw.replace('"', "\\\""))
    } else {
        raw.into_owned()
    }
}

fn output_executable_path(session: &Session, _input: &Path) -> PathBuf {
    session.opts.output.clone().unwrap_or_else(default_executable_path)
}

fn default_executable_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("a.exe")
    } else {
        PathBuf::from("a.out")
    }
}

static NEXT_TEMP_OBJECT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempObject {
    path: PathBuf,
}

impl TempObject {
    fn new(input: &Path) -> Self {
        let id = NEXT_TEMP_OBJECT_ID.fetch_add(1, Ordering::Relaxed);
        let stem = input.file_stem().and_then(OsStr::to_str).unwrap_or("input");
        let path = env::temp_dir().join(format!("rcc-{}-{id}-{stem}.o", std::process::id()));
        let _ = fs::remove_file(&path);
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempObject {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn stage_output_path(base: &Path, kind: EmitKind) -> PathBuf {
    PathBuf::from(format!("{}.{}", base.display(), stage_extension(kind)))
}

fn stage_extension(kind: EmitKind) -> &'static str {
    match kind {
        EmitKind::Tokens => "tokens",
        EmitKind::Pp => "pp",
        EmitKind::Ast => "ast",
        EmitKind::Hir => "hir",
        EmitKind::Mir => "mir",
        EmitKind::LlvmIr => "ll",
        EmitKind::Asm => "s",
        EmitKind::Obj => "o",
    }
}

fn format_mir(
    tcx: &TyCtxt,
    bodies: &rcc_data_structures::FxHashMap<rcc_hir::DefId, rcc_cfg::Body>,
) -> String {
    let mut out = String::new();
    let mut ids: Vec<_> = bodies.keys().copied().collect();
    ids.sort_by_key(|id| id.0);
    for (idx, id) in ids.iter().enumerate() {
        if idx > 0 {
            out.push('\n');
        }
        if let Some(body) = bodies.get(id) {
            out.push_str(&dump_body(body, tcx));
        }
    }
    out
}

/// Write a human-readable rendering of the preprocessed pp-token
/// stream, one token per space, newlines inserted between
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
fn format_preprocessed(session: &Session, tokens: &[PpToken]) -> String {
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
    buf
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
