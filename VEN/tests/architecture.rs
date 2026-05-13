/// SC-004 — Architecture boundary test: route files must not import `Profile`.
///
/// This test walks all `.rs` files under `VEN/src/routes/` and fails if any
/// contain a direct import of the raw `Profile` configuration type.  It makes
/// the AB-06 boundary permanent and is caught automatically in CI / `cargo test`.
///
/// To verify SC-004 manually:
///   1. Add `use crate::profile::Profile;` to any file in `VEN/src/routes/`
///   2. Run `cargo test architecture` — the test fails with a diagnostic naming the file.
///   3. Remove the import — the test passes.

use std::path::{Path, PathBuf};

fn routes_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join("routes")
}

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(collect_rs_files(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    files
}

#[test]
fn routes_must_not_import_profile() {
    let routes = routes_dir();
    assert!(
        routes.exists(),
        "routes dir not found at {}: check CARGO_MANIFEST_DIR",
        routes.display()
    );

    let files = collect_rs_files(&routes);
    assert!(
        !files.is_empty(),
        "no .rs files found under {} — check test setup",
        routes.display()
    );

    let mut violations: Vec<String> = Vec::new();
    for file in &files {
        let content = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        if content.contains("use crate::profile") || content.contains("crate::profile::") {
            violations.push(file.display().to_string());
        }
    }

    assert!(
        violations.is_empty(),
        "AB-06 VIOLATION: the following route file(s) import the raw `Profile` \
         configuration type, which is prohibited in the routes layer:\n  {}\n\
         Move the value into AppCtx at startup and pass it as a pre-computed field.",
        violations.join("\n  ")
    );
}
