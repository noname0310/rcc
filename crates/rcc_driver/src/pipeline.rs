//! Orchestration of the compiler pipeline: source -> preprocess -> parse ->
//! lower -> typeck -> cfg-build -> cfg-transform -> codegen.

use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rcc_cfg::{build_bodies, pretty::dump_body};
use rcc_codegen_llvm::{codegen, CodegenError};
use rcc_hir::TyCtxt;
use rcc_hir_lower::lower;
use rcc_lexer::{PpToken, PpTokenKind};
use rcc_preprocess::preprocess;
use rcc_session::{EmitKind, LinkOptions, Session};
use rcc_typeck::{check, verify_typed_hir};

pub use crate::toolchain::CommandSpec as LinkCommand;
use crate::toolchain::{CommandSpec, Toolchain};

/// Compile a single file end-to-end. Errors are written to the session's
/// diagnostic handler; this function only returns `Err` for unrecoverable
/// I/O or backend failures.
pub fn compile(session: &mut Session, input: &Path) -> Result<(), String> {
    let output_plan = OutputPlan::new(session, input);
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
            return output_plan.write_stage_outputs(&stage_outputs);
        }
    }

    // 2. Preprocess.
    let pp_tokens = preprocess(session, file);
    let pp_output = if session.opts.emit.contains(&EmitKind::Pp) || output_plan.saves_temps() {
        Some(format_preprocessed(session, &pp_tokens))
    } else {
        None
    };
    if let Some(output) = pp_output.as_ref().filter(|_| output_plan.saves_temps()) {
        output_plan.write_saved_temp(EmitKind::Pp, output.as_bytes())?;
    }
    if session.opts.emit.contains(&EmitKind::Pp) {
        stage_outputs.push(StageOutput::text(
            EmitKind::Pp,
            pp_output.expect("preprocessed output was computed for --emit=pp"),
        ));
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
            return output_plan.write_stage_outputs(&stage_outputs);
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
            return output_plan.write_stage_outputs(&stage_outputs);
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
            return output_plan.write_stage_outputs(&stage_outputs);
        }
    }

    // 6. Build CFG.
    let bodies = build_bodies(session, &tcx, &hir);
    if session.opts.emit.contains(&EmitKind::Mir) {
        stage_outputs.push(StageOutput::text(EmitKind::Mir, format_mir(&tcx, &bodies)));
        if !backend_required(&session.opts.emit) {
            return output_plan.write_stage_outputs(&stage_outputs);
        }
    }

    // 7. Codegen.
    match codegen(session, &tcx, &hir, &bodies) {
        Ok(art) => {
            if output_plan.saves_temps() {
                output_plan.write_saved_temp(EmitKind::LlvmIr, art.ir_text.as_bytes())?;
                if let Some(assembly) = &art.assembly_text {
                    output_plan.write_saved_temp(EmitKind::Asm, assembly.as_bytes())?;
                }
                if let Some(object) = &art.object_bytes {
                    output_plan.write_saved_temp(EmitKind::Obj, object)?;
                }
            }
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
                let exe = output_plan.executable_path()?;
                if let Some(saved_obj) = output_plan.saved_temp_path(EmitKind::Obj)? {
                    return link_with_options(&saved_obj, &exe, &session.opts.link);
                }
                let obj = TempObject::new(input)?;
                obj.write(&object)?;
                return link_with_options(obj.path(), &exe, &session.opts.link);
            }
            if session.opts.emit.contains(&EmitKind::Obj) {
                let object = art
                    .object_bytes
                    .ok_or_else(|| "LLVM backend did not return object output".to_string())?;
                stage_outputs.push(StageOutput::bytes(EmitKind::Obj, object));
            }
            output_plan.write_stage_outputs(&stage_outputs)
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum ArtifactPlan {
    Stdout,
    File(PathBuf),
    StageFile(PathBuf),
    SavedTemp(PathBuf),
    PrivateTemp(PathBuf),
}

impl ArtifactPlan {
    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        match self {
            Self::Stdout => {
                io::stdout().write_all(bytes).map_err(|e| format!("cannot write stdout: {e}"))
            }
            Self::File(path)
            | Self::StageFile(path)
            | Self::SavedTemp(path)
            | Self::PrivateTemp(path) => {
                if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty())
                {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
                }
                fs::write(path, bytes).map_err(|e| format!("cannot write {}: {e}", path.display()))
            }
        }
    }
}

#[derive(Clone, Debug)]
struct OutputPlan {
    input: PathBuf,
    output: Option<PathBuf>,
    save_temps: Option<PathBuf>,
    emit: Vec<EmitKind>,
}

impl OutputPlan {
    fn new(session: &Session, input: &Path) -> Self {
        Self {
            input: input.to_path_buf(),
            output: session.opts.output.clone(),
            save_temps: session.opts.save_temps.clone(),
            emit: session.opts.emit.clone(),
        }
    }

    fn saves_temps(&self) -> bool {
        self.save_temps.is_some()
    }

    fn executable_path(&self) -> Result<PathBuf, String> {
        let output = self.output.clone().unwrap_or_else(default_executable_path);
        self.ensure_output_does_not_clobber_input(&output)?;
        Ok(output)
    }

    fn write_stage_outputs(&self, outputs: &[StageOutput]) -> Result<(), String> {
        if outputs.is_empty() {
            return Ok(());
        }

        if outputs.len() == 1 && self.emit.len() == 1 {
            let artifact = if let Some(path) = &self.output {
                self.ensure_output_does_not_clobber_input(path)?;
                ArtifactPlan::File(path.clone())
            } else {
                ArtifactPlan::Stdout
            };
            return artifact.write(&outputs[0].bytes);
        }

        for output in outputs {
            let artifact = ArtifactPlan::StageFile(self.stage_output_path(output.kind));
            artifact.write(&output.bytes)?;
        }
        Ok(())
    }

    fn write_saved_temp(&self, kind: EmitKind, bytes: &[u8]) -> Result<(), String> {
        let Some(path) = self.saved_temp_path(kind)? else {
            return Ok(());
        };
        ArtifactPlan::SavedTemp(path).write(bytes)
    }

    fn saved_temp_path(&self, kind: EmitKind) -> Result<Option<PathBuf>, String> {
        let Some(dir) = &self.save_temps else {
            return Ok(None);
        };
        let stem = self.input.file_stem().and_then(OsStr::to_str).unwrap_or("input");
        let path = dir.join(format!("{stem}.{}", saved_temp_extension(kind)));
        self.ensure_output_does_not_clobber_input(&path)?;
        Ok(Some(path))
    }

    fn stage_output_path(&self, kind: EmitKind) -> PathBuf {
        let base = self.output.as_deref().unwrap_or(&self.input);
        PathBuf::from(format!("{}.{}", base.display(), stage_extension(kind)))
    }

    fn ensure_output_does_not_clobber_input(&self, output: &Path) -> Result<(), String> {
        if same_file_or_same_path(output, &self.input) {
            Err(format!("refusing to overwrite input file {}", self.input.display()))
        } else {
            Ok(())
        }
    }
}

/// Link one native object file into an executable using clang + LLVM lld.
///
/// This deliberately goes through a clang-compatible linker driver with
/// `-fuse-ld=lld` instead of invoking `ld.lld` directly, so libc and CRT
/// startup objects stay the platform driver's responsibility.
pub fn link(obj: &Path, output: &Path) -> Result<(), String> {
    link_with_options(obj, output, &LinkOptions::default())
}

/// Link one native object file into an executable/shared object with options.
pub fn link_with_options(obj: &Path, output: &Path, options: &LinkOptions) -> Result<(), String> {
    link_objects_with_options(&[obj.to_path_buf()], output, options)
}

/// Link several native object files into one output with options.
pub fn link_objects_with_options(
    objects: &[PathBuf],
    output: &Path,
    options: &LinkOptions,
) -> Result<(), String> {
    let linker_driver = match &options.linker_driver {
        Some(path) => path.clone(),
        None => Toolchain::discover().map_err(|err| err.to_string())?.linker_driver,
    };
    link_objects_with_linker_and_options(&linker_driver, objects, output, options)
}

/// Link with an explicit linker-driver path. Public for driver tests and later
/// tool discovery work; ordinary users should call [`link`].
pub fn link_with_linker(linker: &Path, obj: &Path, output: &Path) -> Result<(), String> {
    link_with_linker_and_options(linker, obj, output, &LinkOptions::default())
}

/// Link with an explicit linker-driver path and explicit forwarding options.
pub fn link_with_linker_and_options(
    linker: &Path,
    obj: &Path,
    output: &Path,
    options: &LinkOptions,
) -> Result<(), String> {
    link_objects_with_linker_and_options(linker, &[obj.to_path_buf()], output, options)
}

/// Link several objects with an explicit linker-driver path and explicit options.
pub fn link_objects_with_linker_and_options(
    linker: &Path,
    objects: &[PathBuf],
    output: &Path,
    options: &LinkOptions,
) -> Result<(), String> {
    let command = CommandSpec::with_objects(linker.to_path_buf(), objects, output, options);
    if options.verbose {
        eprintln!("link command: {}", command.render());
    }
    let result = command
        .to_command()
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

fn default_executable_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("a.exe")
    } else {
        PathBuf::from("a.out")
    }
}

static NEXT_TEMP_OBJECT_ID: AtomicUsize = AtomicUsize::new(0);

struct TempObject {
    dir: PathBuf,
    path: PathBuf,
}

impl TempObject {
    fn new(input: &Path) -> Result<Self, String> {
        let id = NEXT_TEMP_OBJECT_ID.fetch_add(1, Ordering::Relaxed);
        let stem = input.file_stem().and_then(OsStr::to_str).unwrap_or("input");
        let dir = env::temp_dir().join(format!("rcc-{}-{id}-{stem}.tmp", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let path = dir.join(format!("{stem}.o"));
        Ok(Self { dir, path })
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        ArtifactPlan::PrivateTemp(self.path.clone()).write(bytes)
    }
}

impl Drop for TempObject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
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

fn saved_temp_extension(kind: EmitKind) -> &'static str {
    match kind {
        EmitKind::Pp => "i",
        kind => stage_extension(kind),
    }
}

fn same_file_or_same_path(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => absolutize(a) == absolutize(b),
    }
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(path)
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
