//! Target triples and target-dependent C ABI facts.
//!
//! This crate is intentionally small and dependency-free so low-level crates
//! such as `rcc_hir` and `rcc_session` can share one target model without
//! introducing cycles.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// Target triple (LLVM/Clang spelling).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TargetTriple(pub String);

impl TargetTriple {
    /// Build a target triple from a raw string.
    #[must_use]
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    /// Return the raw triple string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TargetTriple {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for TargetTriple {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for TargetTriple {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// C data model family.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum DataModel {
    /// 32-bit `int`, `long`, and pointer.
    Ilp32,
    /// 32-bit `int`, 64-bit `long` and pointer.
    Lp64,
    /// 32-bit `int` and `long`, 64-bit `long long` and pointer.
    Llp64,
}

/// Target byte order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Endianness {
    /// Least-significant byte first.
    Little,
    /// Most-significant byte first.
    Big,
}

/// Target architecture.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Arch {
    /// x86-64 / AMD64.
    X86_64,
    /// AArch64.
    Aarch64,
    /// 32-bit x86.
    I386,
}

/// Target operating system.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Os {
    /// Linux.
    Linux,
    /// Apple Darwin / macOS.
    Darwin,
    /// Microsoft Windows.
    Windows,
    /// Bare-metal or unknown OS.
    None,
}

/// Target ABI/environment component.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Environment {
    /// GNU userspace ABI.
    Gnu,
    /// Microsoft Visual C++ ABI.
    Msvc,
    /// No specific environment component.
    Unknown,
}

/// Size and ABI alignment of one C type, in bytes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeLayout {
    /// Object size in bytes.
    pub size: u64,
    /// ABI alignment in bytes.
    pub align: u32,
}

/// Target-dependent scalar and builtin type layouts.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TypeLayouts {
    /// `_Bool`.
    pub bool_: TypeLayout,
    /// `char`, `signed char`, and `unsigned char`.
    pub char_: TypeLayout,
    /// `short`.
    pub short: TypeLayout,
    /// `int`.
    pub int: TypeLayout,
    /// `long`.
    pub long: TypeLayout,
    /// `long long`.
    pub long_long: TypeLayout,
    /// `float`.
    pub float: TypeLayout,
    /// `double`.
    pub double: TypeLayout,
    /// `long double`.
    pub long_double: TypeLayout,
    /// Object/function pointer.
    pub pointer: TypeLayout,
    /// Current `__builtin_va_list` placeholder layout for supported C ABIs.
    pub builtin_va_list: TypeLayout,
}

/// Fully parsed target information needed by frontend layout and codegen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetInfo {
    /// Normalized LLVM target triple.
    pub triple: TargetTriple,
    /// Target architecture.
    pub arch: Arch,
    /// Target operating system.
    pub os: Os,
    /// Target environment / ABI component.
    pub env: Environment,
    /// C data model.
    pub data_model: DataModel,
    /// Pointer width in bits.
    pub pointer_width: u32,
    /// Target byte order.
    pub endianness: Endianness,
    /// Target-dependent C layouts.
    pub layouts: TypeLayouts,
    /// LLVM data layout string for this target baseline.
    pub llvm_data_layout: &'static str,
}

/// Error returned by target triple parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetError {
    triple: String,
    reason: &'static str,
}

impl TargetError {
    fn new(triple: &str, reason: &'static str) -> Self {
        Self { triple: triple.to_owned(), reason }
    }
}

impl std::fmt::Display for TargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported target triple `{}`: {}", self.triple, self.reason)
    }
}

impl std::error::Error for TargetError {}

impl TargetInfo {
    /// rcc's current backend baseline: Linux x86-64 SysV / LP64.
    #[must_use]
    pub fn baseline() -> Self {
        Self::x86_64_linux_gnu("x86_64-unknown-linux-gnu")
    }

    /// Construct target info from a target triple.
    pub fn from_triple(triple: &TargetTriple) -> Result<Self, TargetError> {
        let parsed = ParsedTriple::parse(triple.as_str())?;
        match (parsed.arch, parsed.os, parsed.env) {
            (Arch::X86_64, Os::Linux, Environment::Gnu | Environment::Unknown) => {
                Ok(Self::x86_64_linux_gnu(triple.as_str()))
            }
            (Arch::Aarch64, Os::Linux, Environment::Gnu | Environment::Unknown) => {
                Ok(Self::aarch64_linux_gnu(triple.as_str()))
            }
            (Arch::X86_64, Os::Darwin, Environment::Unknown) => {
                Ok(Self::x86_64_apple_darwin(triple.as_str()))
            }
            (Arch::X86_64, Os::Windows, Environment::Msvc) => {
                Ok(Self::x86_64_windows_msvc(triple.as_str()))
            }
            _ => Err(TargetError::new(triple.as_str(), "combination is not supported yet")),
        }
    }

    /// Construct target info for the host compiler process.
    #[must_use]
    pub fn host() -> Self {
        Self::from_triple(&TargetTriple::new(host_triple())).unwrap_or_else(|_| Self::baseline())
    }

    /// Integer layout by C conversion rank.
    #[must_use]
    pub fn int_layout(&self, rank: IntRankLayout) -> TypeLayout {
        match rank {
            IntRankLayout::Bool => self.layouts.bool_,
            IntRankLayout::Char => self.layouts.char_,
            IntRankLayout::Short => self.layouts.short,
            IntRankLayout::Int => self.layouts.int,
            IntRankLayout::Long => self.layouts.long,
            IntRankLayout::LongLong => self.layouts.long_long,
        }
    }

    /// Floating-point layout by C floating kind.
    #[must_use]
    pub fn float_layout(&self, kind: FloatLayoutKind) -> TypeLayout {
        match kind {
            FloatLayoutKind::Float => self.layouts.float,
            FloatLayoutKind::Double => self.layouts.double,
            FloatLayoutKind::LongDouble => self.layouts.long_double,
        }
    }

    fn x86_64_linux_gnu(triple: &str) -> Self {
        Self::lp64(triple, Arch::X86_64, Os::Linux, Environment::Gnu, X86_64_SYSV_DATALAYOUT)
    }

    fn aarch64_linux_gnu(triple: &str) -> Self {
        Self::lp64(triple, Arch::Aarch64, Os::Linux, Environment::Gnu, AARCH64_LINUX_DATALAYOUT)
    }

    fn x86_64_apple_darwin(triple: &str) -> Self {
        Self::lp64(triple, Arch::X86_64, Os::Darwin, Environment::Unknown, X86_64_DARWIN_DATALAYOUT)
    }

    fn x86_64_windows_msvc(triple: &str) -> Self {
        let layouts = TypeLayouts {
            long: TypeLayout { size: 4, align: 4 },
            long_double: TypeLayout { size: 8, align: 8 },
            builtin_va_list: TypeLayout { size: 8, align: 8 },
            ..lp64_layouts()
        };
        Self {
            triple: TargetTriple::new(triple),
            arch: Arch::X86_64,
            os: Os::Windows,
            env: Environment::Msvc,
            data_model: DataModel::Llp64,
            pointer_width: 64,
            endianness: Endianness::Little,
            layouts,
            llvm_data_layout: X86_64_WINDOWS_MSVC_DATALAYOUT,
        }
    }

    fn lp64(
        triple: &str,
        arch: Arch,
        os: Os,
        env: Environment,
        llvm_data_layout: &'static str,
    ) -> Self {
        Self {
            triple: TargetTriple::new(triple),
            arch,
            os,
            env,
            data_model: DataModel::Lp64,
            pointer_width: 64,
            endianness: Endianness::Little,
            layouts: lp64_layouts(),
            llvm_data_layout,
        }
    }
}

/// Integer rank mirror used to avoid depending on `rcc_hir`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum IntRankLayout {
    /// `_Bool`.
    Bool,
    /// `char`.
    Char,
    /// `short`.
    Short,
    /// `int`.
    Int,
    /// `long`.
    Long,
    /// `long long`.
    LongLong,
}

/// Floating type mirror used to avoid depending on `rcc_hir`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FloatLayoutKind {
    /// `float`.
    Float,
    /// `double`.
    Double,
    /// `long double`.
    LongDouble,
}

const X86_64_SYSV_DATALAYOUT: &str =
    "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128";
const AARCH64_LINUX_DATALAYOUT: &str = "e-m:e-i8:8:32-i16:16:32-i64:64-i128:128-n32:64-S128";
const X86_64_DARWIN_DATALAYOUT: &str = "e-m:o-i64:64-f80:128-n8:16:32:64-S128";
const X86_64_WINDOWS_MSVC_DATALAYOUT: &str =
    "e-m:w-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128";

fn lp64_layouts() -> TypeLayouts {
    TypeLayouts {
        bool_: TypeLayout { size: 1, align: 1 },
        char_: TypeLayout { size: 1, align: 1 },
        short: TypeLayout { size: 2, align: 2 },
        int: TypeLayout { size: 4, align: 4 },
        long: TypeLayout { size: 8, align: 8 },
        long_long: TypeLayout { size: 8, align: 8 },
        float: TypeLayout { size: 4, align: 4 },
        double: TypeLayout { size: 8, align: 8 },
        long_double: TypeLayout { size: 16, align: 16 },
        pointer: TypeLayout { size: 8, align: 8 },
        builtin_va_list: TypeLayout { size: 24, align: 8 },
    }
}

struct ParsedTriple {
    arch: Arch,
    os: Os,
    env: Environment,
}

impl ParsedTriple {
    fn parse(raw: &str) -> Result<Self, TargetError> {
        let parts = raw.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>();
        if parts.len() < 2 {
            return Err(TargetError::new(raw, "expected at least arch-os"));
        }
        let arch = parse_arch(parts[0]).ok_or_else(|| TargetError::new(raw, "unknown arch"))?;
        let (os_part, env_part) = match parts.as_slice() {
            [_, os] => (*os, None),
            [_, maybe_vendor, maybe_os] if parse_os(maybe_vendor).is_some() => {
                (*maybe_vendor, Some(*maybe_os))
            }
            [_, _, os] => (*os, None),
            [_, _, os, env, ..] => (*os, Some(*env)),
            _ => return Err(TargetError::new(raw, "malformed triple")),
        };
        let os = parse_os(os_part).ok_or_else(|| TargetError::new(raw, "unknown os"))?;
        let env = env_part.and_then(parse_env).unwrap_or(Environment::Unknown);
        Ok(Self { arch, os, env })
    }
}

fn parse_arch(raw: &str) -> Option<Arch> {
    match raw {
        "x86_64" | "amd64" => Some(Arch::X86_64),
        "aarch64" | "arm64" => Some(Arch::Aarch64),
        "i386" | "i486" | "i586" | "i686" => Some(Arch::I386),
        _ => None,
    }
}

fn parse_os(raw: &str) -> Option<Os> {
    match raw {
        "linux" => Some(Os::Linux),
        "darwin" | "macos" => Some(Os::Darwin),
        "windows" | "win32" => Some(Os::Windows),
        "none" | "unknown" => Some(Os::None),
        _ => None,
    }
}

fn parse_env(raw: &str) -> Option<Environment> {
    match raw {
        "gnu" | "gnueabi" | "gnueabihf" => Some(Environment::Gnu),
        "msvc" => Some(Environment::Msvc),
        "unknown" => Some(Environment::Unknown),
        _ => None,
    }
}

fn host_triple() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("x86_64", "windows") => "x86_64-pc-windows-msvc",
        _ => "x86_64-unknown-linux-gnu",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_x86_64_is_lp64() {
        let info = TargetInfo::from_triple(&TargetTriple::new("x86_64-unknown-linux-gnu")).unwrap();
        assert_eq!(info.pointer_width, 64);
        assert_eq!(info.data_model, DataModel::Lp64);
        assert_eq!(info.layouts.long, TypeLayout { size: 8, align: 8 });
        assert_eq!(info.layouts.pointer, TypeLayout { size: 8, align: 8 });
    }

    #[test]
    fn windows_x86_64_is_llp64() {
        let info = TargetInfo::from_triple(&TargetTriple::new("x86_64-pc-windows-msvc")).unwrap();
        assert_eq!(info.pointer_width, 64);
        assert_eq!(info.data_model, DataModel::Llp64);
        assert_eq!(info.layouts.long, TypeLayout { size: 4, align: 4 });
        assert_eq!(info.layouts.long_long, TypeLayout { size: 8, align: 8 });
    }

    #[test]
    fn aarch64_linux_is_supported() {
        let info =
            TargetInfo::from_triple(&TargetTriple::new("aarch64-unknown-linux-gnu")).unwrap();
        assert_eq!(info.arch, Arch::Aarch64);
        assert_eq!(info.os, Os::Linux);
        assert_eq!(info.data_model, DataModel::Lp64);
    }

    #[test]
    fn malformed_or_unsupported_triple_is_error() {
        assert!(TargetInfo::from_triple(&TargetTriple::new("not-a-real-target")).is_err());
        assert!(TargetInfo::from_triple(&TargetTriple::new("i386-unknown-linux-gnu")).is_err());
    }
}
