  ---
  Phase 7 spec — Type the VTN report interface

  Feature ID: 025-type-vtn-report
  One-line goal: Replace every serde_json::Value on the public report-submission surface with typed OpenADR 3
  structs so that VtnPort::upsert_report and all callers are fully type-safe.

  ---
  Context

  Phases 1–6 of docs/plans/ven_backend_architecture_refactoring.md addressed AB-01 through AB-06. Phase 7 is
  the final structural gap: the report body passed to VtnPort::upsert_report is still serde_json::Value. All
  three public callers — tasks/planning.rs, tasks/sim_tick/publish.rs, and services/obligation.rs — receive an   Option<serde_json::Value> from controller/reporter.rs and pass it straight to vtn.upsert_report. The
  VtnPort trait in controller/vtn_port.rs was introduced in 024; the typed structs for programs, events, and
  reports exist there already, but the upsert_report method was deliberately deferred.

  ---
  Success criteria (verifiable)

  - grep "serde_json::Value" VEN/src/vtn.rs — zero matches on non-private function signatures (private helpers   get_json, post_json, put_json, post_json_raw may keep their internal Value returns)
  - grep "serde_json::Value" VEN/src/controller/vtn_port.rs — zero matches on VtnPort trait methods
  - grep "serde_json::Value" VEN/src/controller/reporter.rs — zero matches on public function signatures
  - All existing tests remain green (wsl cargo test -p ven)

  ---
  New typed structs (add to controller/vtn_port.rs)

  The report body shape currently produced by reporter.rs via json!{} macros:

  {
    "programID": "...",
    "eventID":   "...",       // optional in status reports
    "clientName": "...",
    "reportName": "...",
    "resources": [{
      "resourceName": "...",
      "intervals": [{
        "id": 0,
        "intervalPeriod": { "start": "...", "duration": "..." },  // optional
        "payloads": [{ "type": "USAGE", "values": [1234.5] }]
      }]
    }]
  }

  Define these structs (OpenADR field names verbatim, per project dto rule):

  pub struct OadrReportBody {
      pub programID: String,
      pub eventID: Option<String>,
      pub clientName: String,
      pub reportName: String,
      pub resources: Vec<OadrReportResource>,
  }

  pub struct OadrReportResource {
      pub resourceName: String,
      pub intervals: Vec<OadrReportInterval>,
  }

  pub struct OadrReportInterval {
      pub id: usize,
      pub intervalPeriod: Option<OadrIntervalPeriod>,  // reuse existing type
      pub payloads: Vec<OadrReportPayload>,
  }

  pub struct OadrReportPayload {
      pub r#type: String,
      pub values: Vec<serde_json::Value>,  // mixed-type array (numbers, strings)
  }

  OadrReportPayload::values may keep Vec<serde_json::Value> — the values array is heterogeneous by OpenADR
  spec design (numbers for power, strings for SoC/state). This is internal structure, not a port signature.

  ---
  Changes required

  controller/vtn_port.rs
  - Add OadrReportBody, OadrReportResource, OadrReportInterval, OadrReportPayload structs (derive Debug,
  Clone, Serialize, Deserialize)
  - Change VtnPort::upsert_report signature: async fn upsert_report(&self, body: OadrReportBody) -> Result<()>   — the return type simplifies to Result<()> since callers only check for errors, not the response body
  - Add contract tests: OadrReportBody round-trips through serde_json::to_value / serde_json::from_value

  vtn.rs (VtnClient)
  - Change VtnClient::upsert_report (inherent, pub(crate)) to accept OadrReportBody, serialize to
  serde_json::Value internally before passing to post_json
  - Update VtnPort for VtnClient impl to match new trait signature
  - Remove submit_report if it becomes unused; otherwise type it identically
  - update_report inherent method: retain serde_json::Value signature (called only from routes/reports.rs and
  internally — it is not on VtnPort)

  controller/reporter.rs
  - Change public function return types:
    - build_measurement_report → Option<OadrReportBody>
    - build_measurement_reports_for_active_events → Vec<OadrReportBody>
    - build_measurement_report_for_obligation → Option<OadrReportBody>
    - build_status_report → Option<OadrReportBody>
  - Replace all json!{} construction with direct struct construction
  - Update internal tests: access fields by name instead of report["resources"][0]["intervals"] etc.

  tasks/planning.rs, tasks/sim_tick/publish.rs, services/obligation.rs
  - Update call sites to pass OadrReportBody directly (no .unwrap() pattern change, just type change)

  services/test_support/mock_vtn.rs
  - Update MockVtn::upsert_report to match new trait signature (OadrReportBody in, Result<()> out)
  - Update captured-body storage type accordingly

  routes/reports.rs
  - post_reports: change Json(body): Json<serde_json::Value> to Json(body): Json<OadrReportBody> — typed
  deserialization from HTTP request
  - put_report: can stay serde_json::Value since it calls VtnClient::update_report (not on VtnPort)

  ---
  Out of scope

  - VtnClient::update_report typing (not on VtnPort; put_report is a passthrough endpoint)
  - Internal get_json/post_json/put_json helpers in vtn.rs
  - Any new HTTP routes or behaviour changes
  - Dependency on Phase 5 (services layer) — this phase is independent

  ---
  Test obligations

  Per docs/plans/ven_backend_architecture_refactoring.md §6:
  - VtnAdapter contract tests: OadrReportBody serializes to the expected JSON shape (fixture assertion)
  - At least one test per reporter.rs public function verifying the returned OadrReportBody fields directly
  (not via ["key"] indexing)

  ---
  Estimated effort

  1–2 days. No logic changes — pure structural typing pass.
