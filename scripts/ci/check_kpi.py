#!/usr/bin/env python3
"""Check conformance KPI targets for the current milestone.

Reads docs/conformance.json and docs/milestone.txt, then verifies that
every required suite meets its pass-rate threshold.  Exits non-zero on
any violation so CI can gate merges.

Pass-rate formula: (pass + xfail) / discovered * 100
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

# ---------------------------------------------------------------------------
# KPI matrix -- numeric pass-rate thresholds per (milestone, suite).
# Only suites with a hard percentage gate appear; text-only targets
# (e.g. "parse-only: small corpus boots") are not machine-checkable here.
# ---------------------------------------------------------------------------
KPI: dict[str, dict[str, float]] = {
    "M0.5": {},
    "M1": {},
    "M2": {},
    "M3": {"c-testsuite": 40.0},
    "M4": {"c-testsuite": 70.0},
    "M5": {"c-testsuite": 80.0},
    "M6": {"c-testsuite": 95.0, "gcc-torture": 60.0},
    "M7": {"c-testsuite": 95.0, "gcc-torture": 70.0},
}


def load_milestone(path: Path) -> str:
    text = path.read_text().strip()
    if text not in KPI:
        print(f"FAIL: unknown milestone '{text}' in {path}", file=sys.stderr)
        sys.exit(1)
    return text


def load_report(path: Path) -> dict:
    if not path.exists():
        print(f"FAIL: report file not found: {path}", file=sys.stderr)
        sys.exit(1)
    with path.open() as f:
        return json.load(f)


def pass_rate(suite: dict) -> float:
    cases = suite.get("cases", {})
    discovered = len(cases)
    if discovered == 0:
        return 0.0
    passing = sum(
        1
        for c in cases.values()
        if c.get("status") in ("pass", "xfail")
    )
    return passing / discovered * 100.0


def main() -> int:
    repo = Path(__file__).resolve().parent.parent.parent
    milestone_path = repo / "docs" / "milestone.txt"
    report_path = repo / "docs" / "conformance.json"

    milestone = load_milestone(milestone_path)
    targets = KPI[milestone]

    if not targets:
        print(f"OK: milestone {milestone} has no numeric KPI requirements.")
        return 0

    report = load_report(report_path)

    suite_map: dict[str, dict] = {}
    for s in report.get("suites", []):
        suite_map[s["name"]] = s

    failures: list[str] = []
    for suite_name, threshold in sorted(targets.items()):
        suite = suite_map.get(suite_name)
        if suite is None:
            failures.append(
                f"  {suite_name}: MISSING from report (need >= {threshold:.1f}%)"
            )
            continue
        actual = pass_rate(suite)
        if actual < threshold:
            failures.append(
                f"  {suite_name}: {actual:.1f}% < {threshold:.1f}% required"
            )
        else:
            print(f"  {suite_name}: {actual:.1f}% >= {threshold:.1f}% OK")

    if failures:
        print(f"FAIL: milestone {milestone} KPI check failed:", file=sys.stderr)
        for f in failures:
            print(f, file=sys.stderr)
        return 1

    print(f"OK: all KPI targets met for milestone {milestone}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
