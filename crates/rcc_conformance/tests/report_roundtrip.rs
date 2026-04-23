//! Integration test: `Report` serialises to JSON and deserialises back
//! to an identical value (serde round-trip).

use std::collections::BTreeMap;

use rcc_conformance::{Outcome, Report, SuiteReport};

#[test]
fn report_roundtrip_json() {
    let report = sample_report();

    let json = report.to_json_pretty();
    let parsed: Report = serde_json::from_str(&json).expect("Report JSON should deserialise back");

    assert_eq!(report.suites.len(), parsed.suites.len());

    for (orig, rt) in report.suites.iter().zip(parsed.suites.iter()) {
        assert_eq!(orig.name, rt.name);
        assert_eq!(orig.cases.len(), rt.cases.len());
        for (id, outcome) in &orig.cases {
            let rt_outcome = rt.cases.get(id).unwrap_or_else(|| {
                panic!("case `{id}` missing after round-trip");
            });
            assert_eq!(outcome, rt_outcome, "mismatch for case `{id}`");
        }
    }
}

#[test]
fn report_counts_agree() {
    let report = sample_report();
    let suite = &report.suites[0];
    let c = suite.counts();

    assert_eq!(c.pass, 1);
    assert_eq!(c.fail, 1);
    assert_eq!(c.xfail, 1);
    assert_eq!(c.skip, 1);
}

#[test]
fn empty_report_roundtrips() {
    let report = Report::default();
    let json = report.to_json_pretty();
    let parsed: Report = serde_json::from_str(&json).unwrap();
    assert!(parsed.suites.is_empty());
}

fn sample_report() -> Report {
    let mut cases = BTreeMap::new();
    cases.insert("test::pass".to_owned(), Outcome::Pass);
    cases.insert("test::fail".to_owned(), Outcome::Fail { reason: "stdout mismatch".to_owned() });
    cases.insert(
        "test::xfail".to_owned(),
        Outcome::XFail { reason: "bit-fields not implemented".to_owned() },
    );
    cases.insert("test::skip".to_owned(), Outcome::Skip { reason: "no .expected file".to_owned() });

    Report { suites: vec![SuiteReport { name: "mock-suite".to_owned(), cases }] }
}
