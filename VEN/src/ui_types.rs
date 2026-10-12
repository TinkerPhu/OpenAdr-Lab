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
    // Plan and planner (increment 2)
    crate::entities::plan::Plan,
    crate::entities::history::PlanHistorySample,
    crate::planner_events::PlannerEvent,
    // Simulator and assets (increment 3)
    crate::controller::sim_snapshot::SimSnapshot,
    crate::entities::sensor::SensorSnapshot,
    crate::entities::sim_inject::SimInjectState,
    crate::assets::ControlDescriptor,
    crate::entities::asset::AssetCapabilityView,
    crate::entities::design_vocabulary::AssetForecast,
    crate::entities::asset_ledger::AssetLedgerEntry,
    crate::routes::hems::AssetLedgerHistory,
    crate::routes::hems::ComfortCurveResponse,
    // System, signals, history, weather, measurement, notifications, arbiter, EV, flexibility
    // (increment 4)
    crate::routes::system::HealthResponse,
    crate::routes::system::VtnStatusResponse,
    crate::routes::system::TaskStatusEntry,
    crate::state::EventLogEntry,
    crate::entities::report_submission::ReportSubmissionRecord,
    crate::routes::reports::ReportWindowSize,
    crate::entities::capacity::OadrReportObligation,
    crate::controller::trace::ControllerEvent,
    crate::entities::tariff_snapshot::TariffSnapshot,
    crate::entities::capacity::CapacitySnapshot,
    crate::routes::hems::SignalsState,
    crate::entities::notification::UserNotification,
    crate::entities::ev_settings::EvSettings,
    crate::entities::ev_usage::EvUsageSimState,
    crate::routes::hems::ArbiterSettings,
    crate::state::ArbiterDiagnostics,
    crate::entities::device_session::BaselineOverride,
    crate::entities::plan::SiteFlexibilityEnvelope,
    crate::entities::plan::SiteFlexibilitySample,
    crate::entities::plan::SiteFlexibilityForecastSlot,
    crate::entities::capacity_curve::CapacityCurves,
    crate::routes::weather::WeatherResponse,
    crate::routes::measurement::MeasurementResponse,
    crate::entities::history::HistoryPage<crate::entities::history::TickSample>,
    crate::entities::history::GridSample,
    crate::entities::history::ForecastAccuracySample,
    crate::entities::history::EventReceived,
    crate::entities::history::ReportSent,
    // Request bodies the UI sends (increment 5)
    crate::routes::hems::CreateUserRequestBody,
    crate::routes::hems::UpdateEvSettingsBody,
    crate::routes::hems::UpdateArbiterSettingsBody,
    crate::routes::hems::CreateBaselineOverrideBody,
    crate::routes::hems::ev_conflict::EvConflictBody,
    crate::routes::error::ErrorBody,
    // Timeline bodies (increment 6)
    crate::routes::timeline::AllTimelinesResponse,
];

/// Where the UI imports the generated files from.
fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/src/api/generated")
}

/// The wire spelling: a 64-bit integer is a JSON number the UI reads as `number`, not `bigint`.
fn config(out_dir: &Path) -> Config {
    Config::new().with_large_int("number").with_out_dir(out_dir)
}

/// Relative path -> content of every `.ts` file under `dir`, subdirectories included (ts-rs puts
/// a foreign type such as `serde_json::Value` in its own folder). Empty when `dir` does not exist.
fn ts_files(dir: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for path in entries.filter_map(Result::ok).map(|e| e.path()) {
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|x| x == "ts") {
                let name = path
                    .strip_prefix(dir)
                    .unwrap()
                    .to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/");
                out.insert(name, std::fs::read_to_string(&path).unwrap());
            }
        }
    }
    out
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
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
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
