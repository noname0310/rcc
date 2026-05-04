//! Warning filtering and promotion policy.

use std::collections::BTreeSet;

use crate::codes;

/// User-selected warning policy.
///
/// The compiler emits warnings by default. This policy models the command-line
/// controls around those diagnostics: global suppression (`-w`), group flags
/// (`-Wall`, `-Wextra`, `-Wpedantic`), named suppression (`-Wno-name`), and
/// warning-to-error promotion (`-Werror`, `-Werror=name`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WarningConfig {
    suppress_all: bool,
    wall: bool,
    extra: bool,
    pedantic: bool,
    warnings_as_errors: bool,
    enabled: BTreeSet<String>,
    disabled: BTreeSet<String>,
    error: BTreeSet<String>,
    no_error: BTreeSet<String>,
}

impl WarningConfig {
    /// Suppress every warning (`-w`).
    pub fn suppress_all(&mut self) {
        self.suppress_all = true;
    }

    /// Whether every warning is suppressed.
    #[must_use]
    pub fn suppresses_all(&self) -> bool {
        self.suppress_all
    }

    /// Enable the `-Wall` group marker.
    pub fn enable_wall(&mut self) {
        self.wall = true;
    }

    /// Whether `-Wall` was requested.
    #[must_use]
    pub fn wall_enabled(&self) -> bool {
        self.wall
    }

    /// Enable the `-Wextra` group marker.
    pub fn enable_extra(&mut self) {
        self.extra = true;
    }

    /// Whether `-Wextra` was requested.
    #[must_use]
    pub fn extra_enabled(&self) -> bool {
        self.extra
    }

    /// Enable the `-Wpedantic` group marker.
    pub fn enable_pedantic(&mut self) {
        self.pedantic = true;
    }

    /// Whether `-Wpedantic` was requested.
    #[must_use]
    pub fn pedantic_enabled(&self) -> bool {
        self.pedantic
    }

    /// Promote every emitted warning to an error (`-Werror`).
    pub fn set_warnings_as_errors(&mut self, value: bool) {
        self.warnings_as_errors = value;
    }

    /// Whether all warnings are promoted to errors.
    #[must_use]
    pub fn warnings_as_errors(&self) -> bool {
        self.warnings_as_errors
    }

    /// Enable a named warning (`-Wname`).
    pub fn enable_warning(&mut self, name: &str) {
        let name = normalize_warning_name(name);
        self.disabled.remove(&name);
        self.enabled.insert(name);
    }

    /// Disable a named warning (`-Wno-name`).
    pub fn disable_warning(&mut self, name: &str) {
        let name = normalize_warning_name(name);
        self.enabled.remove(&name);
        self.disabled.insert(name);
    }

    /// Promote a named warning to an error (`-Werror=name`).
    pub fn promote_warning(&mut self, name: &str) {
        let name = normalize_warning_name(name);
        self.no_error.remove(&name);
        self.error.insert(name);
    }

    /// Stop promoting a named warning (`-Wno-error=name`).
    pub fn demote_warning(&mut self, name: &str) {
        let name = normalize_warning_name(name);
        self.error.remove(&name);
        self.no_error.insert(name);
    }

    /// Whether a named warning override exists for `name`.
    #[must_use]
    pub fn warning_disabled(&self, name: &str) -> bool {
        self.disabled.contains(&normalize_warning_name(name))
    }

    /// Return whether a diagnostic warning should be emitted.
    #[must_use]
    pub fn should_emit_warning(&self, code: Option<&str>) -> bool {
        if self.suppress_all {
            return false;
        }
        !self.disabled.iter().any(|name| warning_code_matches_name(code, name))
    }

    /// Return whether a diagnostic warning should be promoted to an error.
    #[must_use]
    pub fn promotes_warning_to_error(&self, code: Option<&str>) -> bool {
        if self.no_error.iter().any(|name| warning_code_matches_name(code, name)) {
            return false;
        }
        self.warnings_as_errors
            || self.error.iter().any(|name| warning_code_matches_name(code, name))
    }
}

fn normalize_warning_name(name: &str) -> String {
    let mut name = name.trim();
    if let Some(stripped) = name.strip_prefix("-W") {
        name = stripped;
    }
    name.trim_start_matches("no-").replace('_', "-").to_ascii_lowercase()
}

fn warning_code_matches_name(code: Option<&str>, name: &str) -> bool {
    let Some(code) = code else {
        return false;
    };
    let normalized = normalize_warning_name(name);
    normalize_warning_name(code) == normalized
        || warning_names_for_code(code).iter().any(|candidate| *candidate == normalized)
}

fn warning_names_for_code(code: &str) -> &'static [&'static str] {
    match code {
        codes::W0001 => &["unknown-pragma"],
        codes::W0002 => &["float-overflow"],
        codes::W0003 => &["multichar", "multi-character-constant"],
        codes::W0004 => &["duplicate-decl-specifier", "duplicate-qualifier"],
        codes::W0005 => &["old-style-definition"],
        codes::W0006 => &["macro-redefined"],
        codes::W0007 => &["enum-overflow"],
        codes::W0008 => &["conversion", "narrowing-conversion"],
        codes::W0009 => &["constant-overflow"],
        codes::W0010 => &["division-by-zero"],
        codes::W0011 => &["shift-count-overflow"],
        codes::W0012 => &["complex-to-real"],
        codes::W0013 => &["gnu-statement-expression"],
        codes::W0014 => &["gnu-range-designator"],
        codes::W0015 => &["gnu-attributes"],
        codes::W0016 => &["gnu-inline-asm"],
        codes::W0017 => &["gnu-omitted-conditional-operand", "gnu-omitted-conditional"],
        codes::W0018 => &["gnu-conditional-void-operand", "gnu-conditional-void"],
        codes::W0019 => &["gnu-case-ranges", "gnu-case-range"],
        codes::W0020 => &["gnu-labels-as-values", "gnu-computed-goto"],
        codes::W0021 => &["gnu-lvalue-comma"],
        codes::W0022 => &["gnu-function-names", "gnu-function-name", "gnu-function"],
        codes::W0023 => &["gnu-va-area", "chibicc-va-area", "va-area"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_warning_matches_code_and_alias() {
        let mut config = WarningConfig::default();
        config.disable_warning("gnu-statement-expression");
        assert!(!config.should_emit_warning(Some(codes::W0013)));

        let mut config = WarningConfig::default();
        config.disable_warning("W0013");
        assert!(!config.should_emit_warning(Some(codes::W0013)));
    }

    #[test]
    fn werror_promotion_can_be_demoted_per_warning() {
        let mut config = WarningConfig::default();
        config.set_warnings_as_errors(true);
        assert!(config.promotes_warning_to_error(Some(codes::W0013)));

        config.demote_warning("gnu-statement-expression");
        assert!(!config.promotes_warning_to_error(Some(codes::W0013)));
    }
}
