use std::path::{Path, PathBuf};

/// Normalize snapshot text so checked-in fixtures remain stable across hosts.
#[must_use]
pub fn normalize_snapshot_text(text: &str) -> String {
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let mut out = text.to_string();
    if let Some(workspace) = workspace {
        let native_root = workspace.display().to_string();
        let slash_root = native_root.replace('\\', "/");
        for root in [native_root, slash_root] {
            let with_backslash =
                if root.ends_with(['\\', '/']) { root.clone() } else { format!("{root}\\") };
            let with_slash = if root.ends_with(['\\', '/']) { root } else { format!("{root}/") };
            out = out.replace(&with_backslash, "");
            out = out.replace(&with_slash, "");
        }
    }
    out
}

/// Assert a driver emit-stage snapshot under `tests/snapshots/<stage>/`.
pub fn assert_emit_snapshot(stage: &str, name: &str, body: impl AsRef<str>) {
    let snapshot_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("snapshots").join(stage);
    let normalized = normalize_snapshot_text(body.as_ref());
    insta::with_settings!({
        snapshot_path => snapshot_path,
        prepend_module_to_snapshot => false,
        omit_expression => true,
    }, {
        insta::assert_snapshot!(name, normalized);
    });
}

macro_rules! assert_emit_snapshot {
    ($stage:expr, $name:expr, $body:expr $(,)?) => {
        $crate::support::assert_emit_snapshot($stage, $name, $body)
    };
}
