//! Render a [`Report`] as a Markdown dashboard table and splice it into
//! `docs/conformance.md` between sentinel markers.

use crate::Report;

const BEGIN_MARKER: &str = "<!-- BEGIN autogen -->";
const END_MARKER: &str = "<!-- END autogen -->";

/// Render the suite-status table from a [`Report`].
///
/// Returns the Markdown fragment (without sentinels) that goes between
/// `<!-- BEGIN autogen -->` and `<!-- END autogen -->`.
pub fn render_dashboard(report: &Report) -> String {
    let mut out = String::new();

    out.push_str(
        "| Suite | Discovered | Pass | XFail | Fail | Skip | % |\n\
         |-------|------------|------|-------|------|------|---|\n",
    );

    for suite in &report.suites {
        let c = suite.counts();
        let discovered = c.pass + c.fail + c.xfail + c.skip;
        let pct = if discovered == 0 {
            0.0
        } else {
            ((c.pass + c.xfail) as f64 / discovered as f64) * 100.0
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {:.1} |\n",
            suite.name, discovered, c.pass, c.xfail, c.fail, c.skip, pct,
        ));
    }

    out
}

/// Replace the content between `<!-- BEGIN autogen -->` and
/// `<!-- END autogen -->` in `markdown` with `generated`.
///
/// Everything outside the sentinels is preserved verbatim.
/// Line endings are normalised to `\n` for cross-platform consistency.
pub fn splice_autogen(markdown: &str, generated: &str) -> anyhow::Result<String> {
    let normalized = markdown.replace("\r\n", "\n");

    let begin = normalized
        .find(BEGIN_MARKER)
        .ok_or_else(|| anyhow::anyhow!("missing `{BEGIN_MARKER}` in conformance.md"))?;
    let end = normalized
        .find(END_MARKER)
        .ok_or_else(|| anyhow::anyhow!("missing `{END_MARKER}` in conformance.md"))?;

    if end < begin {
        anyhow::bail!("`{END_MARKER}` appears before `{BEGIN_MARKER}`");
    }

    let mut result = String::with_capacity(normalized.len());
    result.push_str(&normalized[..begin + BEGIN_MARKER.len()]);
    result.push('\n');
    result.push_str(generated);
    result.push_str(&normalized[end..]);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splice_preserves_surrounding_text() {
        let md =
            "# Title\n\n<!-- BEGIN autogen -->\nold table\n<!-- END autogen -->\n\n## Footer\n";
        let result = splice_autogen(md, "new table\n").unwrap();
        assert!(result.starts_with("# Title\n\n<!-- BEGIN autogen -->\n"));
        assert!(result.contains("new table\n<!-- END autogen -->"));
        assert!(result.ends_with("\n\n## Footer\n"));
    }

    #[test]
    fn splice_errors_on_missing_begin() {
        let md = "no markers here\n<!-- END autogen -->\n";
        assert!(splice_autogen(md, "x").is_err());
    }

    #[test]
    fn splice_errors_on_missing_end() {
        let md = "<!-- BEGIN autogen -->\nno end\n";
        assert!(splice_autogen(md, "x").is_err());
    }
}
