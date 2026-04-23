//! Integration test for the conformance dashboard renderer.

use rcc_conformance::render::{render_dashboard, splice_autogen};
use rcc_conformance::Report;

const FIXTURE_JSON: &str = include_str!("fixtures/render/sample_report.json");
const FIXTURE_INPUT_MD: &str = include_str!("fixtures/render/dashboard_input.md");
const FIXTURE_EXPECTED_MD: &str = include_str!("fixtures/render/dashboard_expected.md");

#[test]
fn render_and_splice_matches_expected() {
    let report: Report = serde_json::from_str(FIXTURE_JSON).expect("fixture JSON should parse");

    let table = render_dashboard(&report);
    let actual = splice_autogen(FIXTURE_INPUT_MD, &table).expect("splice should succeed");

    let expected = FIXTURE_EXPECTED_MD.replace("\r\n", "\n");
    assert_eq!(actual, expected);
}

#[test]
fn render_empty_report_produces_header_only() {
    let report = Report::default();
    let table = render_dashboard(&report);

    assert!(table.starts_with("| Suite |"));
    assert!(table.contains("|-------|"));
    let lines: Vec<&str> = table.lines().collect();
    assert_eq!(lines.len(), 2, "empty report should have only the header rows");
}

#[test]
fn percentage_zero_discovered() {
    use rcc_conformance::SuiteReport;
    use std::collections::BTreeMap;

    let report =
        Report { suites: vec![SuiteReport { name: "empty".to_owned(), cases: BTreeMap::new() }] };
    let table = render_dashboard(&report);

    assert!(table.contains("| empty | 0 | 0 | 0 | 0 | 0 | 0.0 |"));
}

#[test]
fn intro_text_preserved_after_splice() {
    let report: Report = serde_json::from_str(FIXTURE_JSON).expect("fixture JSON should parse");
    let table = render_dashboard(&report);
    let actual = splice_autogen(FIXTURE_INPUT_MD, &table).expect("splice should succeed");

    assert!(actual.contains("Hand-written intro text that must be preserved."));
    assert!(actual.contains("This section must also be preserved."));
}
