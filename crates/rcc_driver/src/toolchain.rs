//! LLVM-oriented tool discovery and command-line rendering.
//!
//! `rcc` owns C front-end compilation and LLVM object emission. Final
//! executable/shared-object linking is delegated to a clang-compatible linker
//! driver with `-fuse-ld=lld`, mirroring Clang's split between driver logic and
//! the LLVM `lld` linker implementation.

use std::collections::BTreeMap;
use std::env;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

use rcc_session::LinkOptions;

/// A command line that has not been spawned yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    program: PathBuf,
    args: Vec<OsString>,
}

impl CommandSpec {
    /// Build `<linker-driver> -fuse-ld=lld <obj> -o <output>`.
    #[must_use]
    pub fn new(program: PathBuf, obj: &Path, output: &Path) -> Self {
        Self::with_options(program, obj, output, &LinkOptions::default())
    }

    /// Build a clang-compatible link command with forwarded linker options.
    #[must_use]
    pub fn with_options(
        program: PathBuf,
        obj: &Path,
        output: &Path,
        options: &LinkOptions,
    ) -> Self {
        Self::with_objects(program, &[obj.to_path_buf()], output, options)
    }

    /// Build a clang-compatible link command for several object files.
    #[must_use]
    pub fn with_objects(
        program: PathBuf,
        objects: &[PathBuf],
        output: &Path,
        options: &LinkOptions,
    ) -> Self {
        let mut args = Vec::new();
        if options.use_lld {
            args.push(OsString::from("-fuse-ld=lld"));
        }
        args.extend(objects.iter().map(|obj| obj.as_os_str().to_owned()));
        args.push(OsString::from("-o"));
        args.push(output.as_os_str().to_owned());
        if options.shared {
            args.push(OsString::from("-shared"));
        }
        if options.static_link {
            args.push(OsString::from("-static"));
        }
        match options.pie {
            Some(true) => args.push(OsString::from("-pie")),
            Some(false) => args.push(OsString::from("-no-pie")),
            None => {}
        }
        for path in &options.library_paths {
            let mut arg = OsString::from("-L");
            arg.push(path);
            args.push(arg);
        }
        for lib in &options.libraries {
            args.push(OsString::from(format!("-l{lib}")));
        }
        for arg in &options.linker_args {
            args.push(OsString::from(arg));
        }
        Self { program, args }
    }

    /// Program path.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Command arguments.
    #[must_use]
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Convert to [`std::process::Command`] for the single spawn point.
    #[must_use]
    pub fn to_command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        command
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

/// LLVM-oriented tools selected for a driver invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolchain {
    /// Clang-compatible linker driver used to invoke lld and system runtime
    /// startup files.
    pub linker_driver: PathBuf,
    /// LLVM `lld` executable if discoverable. The command still uses
    /// `-fuse-ld=lld` by default so clang can supply CRT/libc paths.
    pub lld: Option<PathBuf>,
    /// LLVM tool prefix selected from `RCC_LLVM_PREFIX` or `LLVM_SYS_181_PREFIX`.
    pub llvm_prefix: Option<PathBuf>,
    /// Object dump/read tool if discoverable.
    pub objdump: Option<PathBuf>,
}

impl Toolchain {
    /// Discover required and optional LLVM-oriented host tools.
    pub fn discover() -> Result<Self, ToolError> {
        Self::discover_with(&ToolFinder::from_env())
    }

    /// Discover tools with an explicit finder. Used by tests.
    pub fn discover_with(finder: &ToolFinder) -> Result<Self, ToolError> {
        Ok(Self {
            linker_driver: finder.find_linker_driver()?,
            lld: finder.find_lld().ok(),
            llvm_prefix: finder.find_llvm_prefix(),
            objdump: finder.find_objdump().ok(),
        })
    }
}

/// Environment-backed LLVM tool lookup service.
#[derive(Clone, Debug, Default)]
pub struct ToolFinder {
    env: BTreeMap<OsString, OsString>,
}

impl ToolFinder {
    /// Capture the current process environment.
    #[must_use]
    pub fn from_env() -> Self {
        Self { env: env::vars_os().collect() }
    }

    /// Build a finder with explicit environment values.
    #[must_use]
    pub fn with_env<I, K, V>(env: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<OsString>,
        V: Into<OsString>,
    {
        Self { env: env.into_iter().map(|(k, v)| (k.into(), v.into())).collect() }
    }

    /// Find clang-compatible linker driver. `rcc` defaults to clang so
    /// `-fuse-ld=lld` selects the LLVM linker while clang supplies platform
    /// CRT/libc details.
    pub fn find_linker_driver(&self) -> Result<PathBuf, ToolError> {
        self.find_required_tool(
            "LLVM linker driver",
            &["RCC_LINKER_DRIVER", "RCC_CLANG", "CLANG"],
            &["clang", "clang-18", "clang-17"],
            &[],
        )
    }

    /// Find the LLVM lld executable.
    pub fn find_lld(&self) -> Result<PathBuf, ToolError> {
        let names = if cfg!(windows) {
            vec!["lld-link", "ld.lld", "lld"]
        } else {
            vec!["ld.lld", "ld.lld-18", "lld"]
        };
        self.find_required_tool("LLVM lld linker", &["RCC_LLD", "LLD"], &names, &[])
    }

    /// Find an LLVM helper tool such as `llvm-readobj`.
    pub fn find_llvm_tool(&self, base: &str) -> Result<PathBuf, ToolError> {
        let names = llvm_tool_names(base);
        let prefix_envs =
            ["RCC_LLVM_PREFIX", "LLVM_SYS_181_PREFIX", "LLVM_SYS_180_PREFIX", "LLVM_PREFIX"];
        let mut searched = Vec::new();
        for env_name in prefix_envs {
            let Some(prefix) = self.non_empty_var(env_name) else {
                continue;
            };
            let bin = PathBuf::from(prefix).join("bin");
            for name in &names {
                let candidate = bin.join(name);
                searched.push(candidate.clone());
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
        self.find_required_tool(
            base,
            &[],
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
            &searched,
        )
    }

    /// Find an object dump/read tool.
    pub fn find_objdump(&self) -> Result<PathBuf, ToolError> {
        if let Some(path) = self.find_override("RCC_OBJDUMP", "object dump tool")? {
            return Ok(path);
        }
        self.find_llvm_tool("llvm-objdump").or_else(|_| {
            self.find_required_tool("object dump tool", &["OBJDUMP"], &["objdump"], &[])
        })
    }

    /// Return a selected LLVM prefix, if any known prefix env var is set.
    #[must_use]
    pub fn find_llvm_prefix(&self) -> Option<PathBuf> {
        ["RCC_LLVM_PREFIX", "LLVM_SYS_181_PREFIX", "LLVM_SYS_180_PREFIX", "LLVM_PREFIX"]
            .into_iter()
            .find_map(|name| self.non_empty_var(name).map(PathBuf::from))
    }

    /// PATH entries used for lookup.
    #[must_use]
    pub fn search_path(&self) -> Vec<PathBuf> {
        self.non_empty_var("PATH").map(env::split_paths).into_iter().flatten().collect()
    }

    fn find_required_tool(
        &self,
        label: &str,
        override_vars: &[&str],
        names: &[&str],
        already_searched: &[PathBuf],
    ) -> Result<PathBuf, ToolError> {
        for var in override_vars {
            if let Some(path) = self.find_override(var, label)? {
                return Ok(path);
            }
        }

        let mut searched = already_searched.to_vec();
        for dir in self.search_path() {
            for name in names.iter().flat_map(|name| self.executable_names(name)) {
                let candidate = dir.join(name);
                searched.push(candidate.clone());
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }

        Err(ToolError::not_found(label, override_vars, names, searched))
    }

    fn find_override(&self, var: &str, label: &str) -> Result<Option<PathBuf>, ToolError> {
        let Some(raw) = self.non_empty_var(var) else {
            return Ok(None);
        };
        let path = PathBuf::from(raw);
        if path.is_file() {
            Ok(Some(path))
        } else {
            Err(ToolError::override_missing(label, var, path))
        }
    }

    fn non_empty_var(&self, name: &str) -> Option<&OsStr> {
        self.env.get(OsStr::new(name)).map(OsString::as_os_str).filter(|value| !value.is_empty())
    }

    fn executable_names(&self, program: &str) -> Vec<OsString> {
        #[cfg(windows)]
        {
            let has_ext = Path::new(program).extension().is_some();
            if has_ext {
                return vec![OsString::from(program)];
            }
            let pathext = self
                .non_empty_var("PATHEXT")
                .map(OsString::from)
                .unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
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
}

/// Tool lookup failure with deterministic diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolError(Box<ToolErrorData>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct ToolErrorData {
    label: String,
    override_var: Option<String>,
    override_path: Option<PathBuf>,
    env_vars: Vec<String>,
    names: Vec<String>,
    searched: Vec<PathBuf>,
}

impl ToolError {
    fn not_found(label: &str, env_vars: &[&str], names: &[&str], searched: Vec<PathBuf>) -> Self {
        Self(Box::new(ToolErrorData {
            label: label.to_owned(),
            override_var: None,
            override_path: None,
            env_vars: env_vars.iter().map(|s| (*s).to_owned()).collect(),
            names: names.iter().map(|s| (*s).to_owned()).collect(),
            searched,
        }))
    }

    fn override_missing(label: &str, var: &str, path: PathBuf) -> Self {
        Self(Box::new(ToolErrorData {
            label: label.to_owned(),
            override_var: Some(var.to_owned()),
            override_path: Some(path.clone()),
            env_vars: vec![var.to_owned()],
            names: Vec::new(),
            searched: vec![path],
        }))
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = &self.0;
        if let (Some(var), Some(path)) = (&data.override_var, &data.override_path) {
            return write!(f, "{} `{}` from {var} was not found", data.label, path.display());
        }
        write!(f, "{} was not found", data.label)?;
        if !data.env_vars.is_empty() {
            write!(f, "; checked env overrides {}", data.env_vars.join(", "))?;
        }
        if !data.names.is_empty() {
            write!(f, "; searched program names {}", data.names.join(", "))?;
        }
        if !data.searched.is_empty() {
            let paths = data
                .searched
                .iter()
                .take(16)
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            write!(f, "; searched paths: {paths}")?;
            if data.searched.len() > 16 {
                write!(f, ", ...")?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for ToolError {}

fn llvm_tool_names(base: &str) -> Vec<String> {
    let suffix = if cfg!(windows) { ".exe" } else { "" };
    vec![format!("{base}{suffix}"), format!("{base}-18{suffix}")]
}

fn quote_arg(arg: &OsStr) -> String {
    let raw = arg.to_string_lossy();
    if raw.is_empty() || raw.chars().any(char::is_whitespace) {
        format!("\"{}\"", raw.replace('"', "\\\""))
    } else {
        raw.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_spec_renders_lld_link_command_without_spawning() {
        let command = CommandSpec::with_options(
            PathBuf::from("clang"),
            Path::new("input.o"),
            Path::new("out"),
            &LinkOptions {
                libraries: vec!["m".to_owned()],
                library_paths: vec![PathBuf::from("/native/lib")],
                linker_args: vec!["-Wl,--as-needed".to_owned()],
                shared: true,
                ..LinkOptions::default()
            },
        );

        let rendered = command.render();
        assert!(rendered.starts_with("clang -fuse-ld=lld"), "{rendered}");
        assert!(rendered.contains("input.o"), "{rendered}");
        assert!(rendered.contains("-o out"), "{rendered}");
        assert!(rendered.contains("-shared"), "{rendered}");
        assert!(rendered.contains("-L/native/lib"), "{rendered}");
        assert!(rendered.contains("-lm"), "{rendered}");
        assert!(rendered.contains("-Wl,--as-needed"), "{rendered}");
    }

    #[test]
    fn missing_linker_driver_reports_overrides_and_search_names() {
        let finder = ToolFinder::with_env([("PATH", "")]);

        let err = finder.find_linker_driver().unwrap_err().to_string();

        assert!(err.contains("LLVM linker driver was not found"), "{err}");
        assert!(err.contains("RCC_LINKER_DRIVER"), "{err}");
        assert!(err.contains("clang"), "{err}");
    }

    #[test]
    fn explicit_missing_linker_driver_mentions_env_var() {
        let finder = ToolFinder::with_env([
            ("RCC_LINKER_DRIVER", "/definitely/missing/clang"),
            ("PATH", ""),
        ]);

        let err = finder.find_linker_driver().unwrap_err().to_string();

        assert!(err.contains("RCC_LINKER_DRIVER"), "{err}");
        assert!(err.contains("/definitely/missing/clang"), "{err}");
    }

    #[test]
    fn llvm_prefix_uses_rcc_override_first() {
        let finder = ToolFinder::with_env([
            ("RCC_LLVM_PREFIX", "/opt/rcc-llvm"),
            ("LLVM_SYS_181_PREFIX", "/opt/llvm-18"),
        ]);

        assert_eq!(finder.find_llvm_prefix(), Some(PathBuf::from("/opt/rcc-llvm")));
    }
}
