//! Diagnostic emitters: where rendered diagnostics go.

use std::collections::HashSet;
use std::io::Write;
use std::sync::{Arc, Mutex, RwLock};

use ariadne::{Color, Config, IndexType, Label as ALabel, Report, ReportKind};

use crate::{Diagnostic, Level};
use rcc_span::SourceMap;

/// Receives fully-built diagnostics and renders them.
pub trait Emitter: Send {
    /// Render a single diagnostic.
    fn emit(&mut self, d: &Diagnostic);
}

/// Ariadne-backed emitter that renders rich diagnostics to stderr.
///
/// Uses `ariadne::Report` / `Source` to produce colourised output with
/// source snippets, underline/caret labels, notes, and help lines.
pub struct StderrEmitter {
    sm: Arc<RwLock<SourceMap>>,
    color: Option<bool>,
}

impl StderrEmitter {
    /// Build a new emitter backed by the given shared source map.
    pub fn new(sm: Arc<RwLock<SourceMap>>) -> Self {
        Self { sm, color: None }
    }

    /// Force colour on or off, overriding `NO_COLOR` env detection.
    pub fn with_color(mut self, color: bool) -> Self {
        self.color = Some(color);
        self
    }

    fn should_colorize(&self) -> bool {
        self.color.unwrap_or_else(|| std::env::var_os("NO_COLOR").is_none())
    }

    fn write_diagnostic<W: Write>(&self, d: &Diagnostic, mut w: W) -> std::io::Result<()> {
        if d.labels.is_empty() {
            return self.write_unspanned(d, &mut w);
        }

        let sm = self.sm.read().unwrap();
        let color = self.should_colorize();

        let kind = match d.level {
            Level::Error | Level::Bug => ReportKind::Error,
            Level::Warning => ReportKind::Warning,
            Level::Note | Level::Help => ReportKind::Advice,
        };

        let primary = d.labels.iter().find(|l| l.primary).unwrap_or(&d.labels[0]);
        let primary_file = sm.file(primary.span.file).name.display().to_string();
        let report_span = (primary_file, primary.span.lo.0 as usize..primary.span.hi.0 as usize);

        let config = Config::default().with_color(color).with_index_type(IndexType::Byte);

        let mut builder =
            Report::build(kind, report_span).with_message(&d.message).with_config(config);

        if let Some(code) = d.code {
            builder = builder.with_code(code);
        }

        for label in &d.labels {
            let fname = sm.file(label.span.file).name.display().to_string();
            let span = (fname, label.span.lo.0 as usize..label.span.hi.0 as usize);
            let mut alabel = ALabel::new(span);
            if !label.message.is_empty() {
                alabel = alabel.with_message(&label.message);
            }
            if label.primary {
                alabel = alabel.with_color(Color::Red);
            } else {
                alabel = alabel.with_color(Color::Blue);
            }
            builder = builder.with_label(alabel);
        }

        for note in &d.notes {
            builder = builder.with_note(note);
        }
        for help in &d.help {
            builder = builder.with_help(help);
        }

        // Collect source text for every referenced file.
        let mut file_sources: Vec<(String, String)> = Vec::new();
        let mut seen = HashSet::new();
        for label in &d.labels {
            if seen.insert(label.span.file) {
                let sf = sm.file(label.span.file);
                file_sources.push((sf.name.display().to_string(), sf.src.to_string()));
            }
        }

        builder.finish().write(ariadne::sources(file_sources), &mut w)
    }

    fn write_unspanned<W: Write>(&self, d: &Diagnostic, w: &mut W) -> std::io::Result<()> {
        let tag = match d.level {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Note => "note",
            Level::Help => "help",
            Level::Bug => "internal compiler error",
        };
        let code = d.code.map(|c| format!("[{c}]")).unwrap_or_default();
        writeln!(w, "{tag}{code}: {}", d.message)?;
        for n in &d.notes {
            writeln!(w, "  = note: {n}")?;
        }
        for h in &d.help {
            writeln!(w, "  = help: {h}")?;
        }
        Ok(())
    }

    /// Render a diagnostic to a `String` (for testing / capture).
    pub fn render_to_string(&self, d: &Diagnostic) -> String {
        let mut buf = Vec::new();
        self.write_diagnostic(d, &mut buf).expect("write to Vec<u8> should not fail");
        String::from_utf8(buf).expect("ariadne output is valid UTF-8")
    }
}

impl Emitter for StderrEmitter {
    fn emit(&mut self, d: &Diagnostic) {
        let stderr = std::io::stderr();
        let _ = self.write_diagnostic(d, stderr.lock());
    }
}

/// Collects every emitted diagnostic into a shared `Vec`. Used by tests.
#[derive(Default, Clone)]
pub struct CaptureEmitter {
    inner: Arc<Mutex<Vec<Diagnostic>>>,
}

impl CaptureEmitter {
    /// Build a new capture emitter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clone of every diagnostic captured so far.
    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.inner.lock().unwrap().clone()
    }
}

impl Emitter for CaptureEmitter {
    fn emit(&mut self, d: &Diagnostic) {
        self.inner.lock().unwrap().push(d.clone());
    }
}
