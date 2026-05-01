//! Suite-level metadata that is not an implementation failure.
//!
//! `xfail.toml` is for known compiler gaps. This module is for fixtures whose
//! expected result is not portable C99 behavior, so differential testing should
//! not compare them against the host compiler's arbitrary choice.

/// Metadata attached to a single test-case id.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CaseMetadata {
    /// Stable `TestCase::id`.
    pub id: &'static str,
    /// Human-readable skip/demotion reason.
    pub reason: &'static str,
}

/// Cases whose stdout depends on unspecified function-argument evaluation
/// order or another C99-unspecified operand order.
///
/// The fixture id here is intentionally tiny and lives in the conformance
/// adapter tests. Real suite ids should be added here only when the entire
/// fixture's expected output is order-dependent and the case cannot be split.
pub const UNSPECIFIED_EVAL_ORDER_CASES: &[CaseMetadata] = &[CaseMetadata {
    id: "chibicc::eval-order",
    reason: "stdout depends on unspecified function-argument evaluation order",
}];

/// Return the skip reason for an unspecified-evaluation-order case.
#[must_use]
pub fn unspecified_eval_order_reason(id: &str) -> Option<&'static str> {
    UNSPECIFIED_EVAL_ORDER_CASES.iter().find_map(|case| (case.id == id).then_some(case.reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_unspecified_eval_order_case_has_reason() {
        let reason = unspecified_eval_order_reason("chibicc::eval-order").unwrap();
        assert!(reason.contains("unspecified"));
    }

    #[test]
    fn ordinary_case_has_no_eval_order_reason() {
        assert_eq!(unspecified_eval_order_reason("chibicc::arith"), None);
    }
}
