//! The VEN UI's API types, generated from the Rust types (R-133).
//!
//! `VEN/ui/src/api/generated/` holds one TypeScript file per type, written by ts-rs from the
//! `#[derive(TS)]` beside each type's `Serialize`; `VEN/ui/src/api/types.ts` re-exports them. This
//! file is the one list of the types the UI reads or sends. Listing a type exports it together with
//! every type it depends on.
//!
//! `generated_ui_types_are_current` fails when the committed files differ from what the Rust types
//! generate today: a field added in Rust and not regenerated is a failing `cargo test`, not a UI
//! that silently lacks it. To regenerate: `UPDATE_UI_TYPES=1 cargo test ui_types`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ts_rs::{Config, ExportError, TS};

/// Every root type the UI reads or sends. ts-rs adds each one's dependencies.
macro_rules! ui_types {
    ($($t:ty),* $(,)?) => {
        fn export_all(cfg: &Config) -> Result<(), ExportError> {
            $( <$t as TS>::export_all(cfg)?; )*
            Ok(())
        }
    };
}

ui_types![
    crate::entities::user_request::UserRequest,
    crate::routes::hems::UserRequestWithSession,
];

/// Where the UI imports the generated files from.
fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/src/api/generated")
}

/// The wire spelling: a 64-bit integer is a JSON number the UI reads as `number`, not `bigint`.
fn config(out_dir: &Path) -> Config {
    Config::new().with_large_int("number").with_out_dir(out_dir)
}

/// File name -> content of every `.ts` file in `dir` (empty when the directory does not exist).
fn ts_files(dir: &Path) -> BTreeMap<String, String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ts"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(&p).unwrap())
        })
        .collect()
}

#[test]
fn generated_ui_types_are_current() {
    let fresh_dir = std::env::temp_dir().join(format!("ven-ui-types-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&fresh_dir);
    export_all(&config(&fresh_dir)).expect("every listed type exports");
    let fresh = ts_files(&fresh_dir);
    let _ = std::fs::remove_dir_all(&fresh_dir);

    let dir = generated_dir();
    if std::env::var_os("UPDATE_UI_TYPES").is_some() {
        for stale in ts_files(&dir).keys() {
            std::fs::remove_file(dir.join(stale)).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        for (name, content) in &fresh {
            std::fs::write(dir.join(name), content).unwrap();
        }
        return;
    }

    let committed = ts_files(&dir);
    let differing: Vec<&String> = fresh
        .keys()
        .chain(committed.keys())
        .filter(|name| fresh.get(*name) != committed.get(*name))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    assert!(
        differing.is_empty(),
        "VEN/ui/src/api/generated/ is stale for {differing:?}; run `UPDATE_UI_TYPES=1 cargo test ui_types`"
    );
}
