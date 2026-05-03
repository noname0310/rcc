//! Stable AST dump used by the driver `--emit=ast` mode.

use crate::TranslationUnit;

/// Dump a translation unit in a deterministic, debug-oriented text format.
#[must_use]
pub fn dump_translation_unit(unit: &TranslationUnit) -> String {
    format!("{unit:#?}\n")
}
