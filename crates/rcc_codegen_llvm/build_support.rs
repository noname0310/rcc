use std::path::{Path, PathBuf};

pub const LLVM_PREFIX_ENV: &str = "LLVM_SYS_181_PREFIX";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlvmCLayout {
    pub prefix: PathBuf,
    pub lib_dir: PathBuf,
    pub bin_dir: PathBuf,
    pub import_lib: PathBuf,
    pub runtime_dll: PathBuf,
}

pub fn validate_llvm_c_prefix(prefix: &Path) -> Result<LlvmCLayout, String> {
    let lib_dir = prefix.join("lib");
    let bin_dir = prefix.join("bin");
    let import_lib = lib_dir.join("LLVM-C.lib");
    let runtime_dll = bin_dir.join("LLVM-C.dll");

    if !prefix.is_dir() {
        return Err(format!(
            "{LLVM_PREFIX_ENV} points to `{}`, but that directory does not exist.\n{}",
            prefix.display(),
            expected_layout()
        ));
    }
    if !import_lib.is_file() {
        return Err(format!("missing `{}`.\n{}", import_lib.display(), expected_layout()));
    }
    if !runtime_dll.is_file() {
        return Err(format!("missing `{}`.\n{}", runtime_dll.display(), expected_layout()));
    }

    Ok(LlvmCLayout { prefix: prefix.to_path_buf(), lib_dir, bin_dir, import_lib, runtime_dll })
}

pub fn expected_layout() -> &'static str {
    "Expected the official LLVM 18 Windows archive layout, e.g.:\n\
     LLVM_SYS_181_PREFIX=D:\\Tools\\clang+llvm-18.1.8-x86_64-pc-windows-msvc\n\
     <prefix>\\lib\\LLVM-C.lib\n\
     <prefix>\\bin\\LLVM-C.dll\n\
     Also put <prefix>\\bin on PATH so LLVM-C.dll is available at test/runtime."
}
