//! Warning filtering and promotion policy.

use std::collections::BTreeSet;

use crate::codes;

/// Stable category for a warning name.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WarningCategory {
    /// Emitted by default unless explicitly suppressed.
    Default,
    /// Enabled by `-Wall` or by its explicit `-Wname` flag.
    Wall,
    /// Enabled by `-Wextra` or by its explicit `-Wname` flag.
    Wextra,
    /// GNU or implementation-extension warning emitted by default in strict mode.
    Extension,
}

struct WarningRecord {
    code: Option<&'static str>,
    names: &'static [&'static str],
    category: WarningCategory,
}

const WALL_WARNING_NAMES: &[&str] =
    &["implicit-function-declaration", "unused-function", "unused-variable"];

const WEXTRA_WARNING_NAMES: &[&str] = &["sign-compare", "unreachable-code", "unused-parameter"];

const WARNING_RECORDS: &[WarningRecord] = &[
    WarningRecord {
        code: Some(codes::W0001),
        names: &["unknown-pragma"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0002),
        names: &["float-overflow"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0003),
        names: &["multichar", "multi-character-constant"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0004),
        names: &["duplicate-decl-specifier", "duplicate-qualifier"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0005),
        names: &["old-style-definition"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0006),
        names: &["macro-redefined"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0007),
        names: &["enum-overflow"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0008),
        names: &["conversion", "narrowing-conversion"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0009),
        names: &["constant-overflow"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0010),
        names: &["division-by-zero"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0011),
        names: &["shift-count-overflow"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0012),
        names: &["complex-to-real"],
        category: WarningCategory::Default,
    },
    WarningRecord {
        code: Some(codes::W0013),
        names: &["gnu-statement-expression"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0014),
        names: &["gnu-range-designator"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0015),
        names: &["gnu-attributes"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0016),
        names: &["gnu-inline-asm"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0017),
        names: &["gnu-omitted-conditional-operand", "gnu-omitted-conditional"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0018),
        names: &["gnu-conditional-void-operand", "gnu-conditional-void"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0019),
        names: &["gnu-case-ranges", "gnu-case-range"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0020),
        names: &["gnu-labels-as-values", "gnu-computed-goto"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0021),
        names: &["gnu-lvalue-comma"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0022),
        names: &["gnu-function-names", "gnu-function-name", "gnu-function"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0023),
        names: &["gnu-va-area", "chibicc-va-area", "va-area"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0024),
        names: &["gnu-typeof", "gnu-typeof-expr", "gnu-typeof-type", "typeof"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: Some(codes::W0025),
        names: &["gnu-alignof", "gnu-alignof-expr", "gnu-alignof-type", "alignof"],
        category: WarningCategory::Extension,
    },
    WarningRecord {
        code: None,
        names: &["implicit-function-declaration"],
        category: WarningCategory::Wall,
    },
    WarningRecord { code: None, names: &["unused-function"], category: WarningCategory::Wall },
    WarningRecord { code: None, names: &["unused-variable"], category: WarningCategory::Wall },
    WarningRecord { code: None, names: &["sign-compare"], category: WarningCategory::Wextra },
    WarningRecord { code: None, names: &["unreachable-code"], category: WarningCategory::Wextra },
    WarningRecord { code: None, names: &["unused-parameter"], category: WarningCategory::Wextra },
];

/// Canonical warning names enabled by `-Wall`.
#[must_use]
pub fn wall_warning_names() -> &'static [&'static str] {
    WALL_WARNING_NAMES
}

/// Canonical warning names enabled by `-Wextra` in addition to `-Wall`.
#[must_use]
pub fn wextra_warning_names() -> &'static [&'static str] {
    WEXTRA_WARNING_NAMES
}

/// Return the category for a warning name or alias.
#[must_use]
pub fn warning_category(name: &str) -> Option<WarningCategory> {
    warning_record_for_name(name).map(|record| record.category)
}

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
        self.disabled.iter().any(|disabled| warning_name_matches(name, disabled))
    }

    /// Whether a named warning is enabled under the current policy.
    ///
    /// Default and extension warnings are enabled unless suppressed. `-Wall`
    /// enables the [`WarningCategory::Wall`] set, and `-Wextra` enables both
    /// the `-Wall` set and the [`WarningCategory::Wextra`] set.
    #[must_use]
    pub fn warning_enabled(&self, name: &str) -> bool {
        if self.suppress_all || self.warning_disabled(name) {
            return false;
        }
        if self.enabled.iter().any(|enabled| warning_name_matches(name, enabled)) {
            return true;
        }
        match warning_category(name) {
            Some(WarningCategory::Default | WarningCategory::Extension) => true,
            Some(WarningCategory::Wall) => self.wall || self.extra,
            Some(WarningCategory::Wextra) => self.extra,
            None => false,
        }
    }

    /// Whether a named warning is promoted to an error under the current policy.
    #[must_use]
    pub fn named_warning_promoted_to_error(&self, name: &str) -> bool {
        if self.no_error.iter().any(|demoted| warning_name_matches(name, demoted)) {
            return false;
        }
        self.warnings_as_errors
            || self.error.iter().any(|promoted| warning_name_matches(name, promoted))
    }

    /// Return whether a diagnostic warning should be emitted.
    #[must_use]
    pub fn should_emit_warning(&self, code: Option<&str>) -> bool {
        if self.suppress_all {
            return false;
        }
        let Some(code) = code else {
            return true;
        };
        if self.disabled.iter().any(|name| warning_code_matches_name(Some(code), name)) {
            return false;
        }
        warning_record_for_code(code).is_none_or(|record| self.warning_enabled(record.names[0]))
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
    warning_record_for_code(code).map_or(&[], |record| record.names)
}

fn warning_record_for_code(code: &str) -> Option<&'static WarningRecord> {
    WARNING_RECORDS.iter().find(|record| record.code == Some(code))
}

fn warning_record_for_name(name: &str) -> Option<&'static WarningRecord> {
    let normalized = normalize_warning_name(name);
    WARNING_RECORDS
        .iter()
        .find(|record| record.names.iter().any(|candidate| *candidate == normalized))
}

fn warning_name_matches(query: &str, configured: &str) -> bool {
    let query = normalize_warning_name(query);
    let configured = normalize_warning_name(configured);
    if query == configured {
        return true;
    }
    match (warning_record_for_name(&query), warning_record_for_name(&configured)) {
        (Some(left), Some(right)) => std::ptr::eq(left, right),
        _ => false,
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

    #[test]
    fn wall_enables_documented_members() {
        let mut config = WarningConfig::default();
        config.enable_wall();

        for name in wall_warning_names() {
            assert!(config.warning_enabled(name), "{name} should be enabled by -Wall");
        }
        for name in wextra_warning_names() {
            assert!(!config.warning_enabled(name), "{name} should not be enabled by -Wall alone");
        }
    }

    #[test]
    fn extra_enables_wall_plus_extra_members() {
        let mut config = WarningConfig::default();
        config.enable_extra();

        for name in wall_warning_names().iter().chain(wextra_warning_names()) {
            assert!(config.warning_enabled(name), "{name} should be enabled by -Wextra");
        }
    }

    #[test]
    fn named_enable_and_disable_override_groups() {
        let mut config = WarningConfig::default();
        config.enable_warning("unused_parameter");
        assert!(config.warning_enabled("unused-parameter"));

        config.enable_wall();
        config.disable_warning("unused-variable");
        assert!(!config.warning_enabled("unused-variable"));
        assert!(config.warning_enabled("unused-function"));
    }

    #[test]
    fn name_based_promotion_respects_aliases_and_demotions() {
        let mut config = WarningConfig::default();
        config.promote_warning("multi-character-constant");
        assert!(config.named_warning_promoted_to_error("multichar"));

        config.set_warnings_as_errors(true);
        config.demote_warning("unused_variable");
        assert!(!config.named_warning_promoted_to_error("unused-variable"));
        assert!(config.named_warning_promoted_to_error("unused-function"));
    }

    #[test]
    fn categories_are_queryable_by_name_and_alias() {
        assert_eq!(warning_category("unused-variable"), Some(WarningCategory::Wall));
        assert_eq!(warning_category("unused-parameter"), Some(WarningCategory::Wextra));
        assert_eq!(warning_category("gnu-statement-expression"), Some(WarningCategory::Extension));
        assert_eq!(warning_category("multi-character-constant"), Some(WarningCategory::Default));
    }
}
