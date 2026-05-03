use std::collections::HashSet;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use rcc_session::{DependencyMode, DependencyOptions, DependencyTarget, Session};

pub(crate) fn emit_dependency_file(
    session: &Session,
    input: &Path,
    default_target: &Path,
) -> Result<(), String> {
    let Some(mode) = session.opts.dependencies.mode else {
        return Ok(());
    };
    if session.handler.has_errors() {
        return Ok(());
    }

    let deps = collect_prerequisites(session, input);
    let targets = dependency_targets(&session.opts.dependencies, default_target);
    let rendered = render_make_rule(&targets, &deps);

    match dependency_output_path(&session.opts.dependencies, input, default_target, mode) {
        Some(path) => {
            if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
                fs::create_dir_all(parent)
                    .map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
            }
            fs::write(&path, rendered)
                .map_err(|err| format!("cannot write dependency file {}: {err}", path.display()))
        }
        None => io::stdout()
            .write_all(rendered.as_bytes())
            .map_err(|err| format!("cannot write stdout: {err}")),
    }
}

pub(crate) fn default_dependency_target(input: &Path, output: Option<&Path>) -> PathBuf {
    output.map(Path::to_path_buf).unwrap_or_else(|| default_object_path(input))
}

fn collect_prerequisites(session: &Session, input: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    push_unique(&mut out, &mut seen, input.to_path_buf());
    for dep in session.source_dependencies() {
        if !session.opts.dependencies.include_system_headers && dep.system {
            continue;
        }
        push_unique(&mut out, &mut seen, dep.path);
    }
    out
}

fn push_unique(out: &mut Vec<PathBuf>, seen: &mut HashSet<PathBuf>, path: PathBuf) {
    if seen.insert(path.clone()) {
        out.push(path);
    }
}

fn dependency_targets(opts: &DependencyOptions, default_target: &Path) -> Vec<String> {
    if opts.targets.is_empty() {
        return vec![escape_make_path(default_target)];
    }
    opts.targets.iter().map(render_user_target).collect()
}

fn render_user_target(target: &DependencyTarget) -> String {
    if target.quote {
        escape_make_text(&target.text)
    } else {
        target.text.clone()
    }
}

fn dependency_output_path(
    opts: &DependencyOptions,
    input: &Path,
    default_target: &Path,
    mode: DependencyMode,
) -> Option<PathBuf> {
    if let Some(path) = &opts.output {
        return Some(path.clone());
    }
    match mode {
        DependencyMode::PreprocessOnly => None,
        DependencyMode::SideEffect => Some(default_dependency_file_path(input, default_target)),
    }
}

fn default_dependency_file_path(input: &Path, default_target: &Path) -> PathBuf {
    let mut path = if default_target.as_os_str().is_empty() {
        input.to_path_buf()
    } else {
        default_target.to_path_buf()
    };
    path.set_extension("d");
    path
}

fn default_object_path(input: &Path) -> PathBuf {
    let mut output = input.to_path_buf();
    output.set_extension("o");
    output
}

fn render_make_rule(targets: &[String], prerequisites: &[PathBuf]) -> String {
    let mut out = String::new();
    out.push_str(&targets.join(" "));
    out.push(':');
    for prereq in prerequisites {
        out.push(' ');
        out.push_str(&escape_make_path(prereq));
    }
    out.push('\n');
    out
}

fn escape_make_path(path: &Path) -> String {
    escape_make_text(&path.to_string_lossy())
}

fn escape_make_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            ' ' | '\t' => {
                out.push('\\');
                out.push(ch);
            }
            '#' => out.push_str("\\#"),
            '$' => out.push_str("$$"),
            '\\' => out.push_str("\\\\"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn make_escaping_covers_spaces_hash_dollar_and_backslash() {
        let raw = r"build dir\a#b$";
        assert_eq!(escape_make_text(raw), r"build\ dir\\a\#b$$");
    }

    #[test]
    fn mt_target_is_literal_and_mq_target_is_escaped() {
        let mt = DependencyTarget { text: "raw target".to_owned(), quote: false };
        let mq = DependencyTarget { text: "quoted target".to_owned(), quote: true };
        assert_eq!(render_user_target(&mt), "raw target");
        assert_eq!(render_user_target(&mq), r"quoted\ target");
    }

    #[test]
    fn default_side_effect_output_uses_target_stem() {
        let input = Path::new("src/hello.c");
        let target = Path::new("obj/hello.o");
        assert_eq!(default_dependency_file_path(input, target), PathBuf::from("obj/hello.d"));
    }
}
