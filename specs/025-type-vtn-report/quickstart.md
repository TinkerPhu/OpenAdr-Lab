# Quickstart: Type the VTN Report Interface

**Feature**: `025-type-vtn-report`  
**Approach**: Compiler-driven — add structs, update trait, let `rustc` guide you to each remaining call site.

---

## Recommended order

This is a pure structural typing pass. The compiler will tell you every place that needs updating once the trait signature changes. Work in this order to keep the build compilable at each step:

### Step 1 — Add the four typed structs to `vtn_port.rs`

Add `OadrReportBody`, `OadrReportResource`, `OadrReportInterval`, `OadrReportPayload` immediately after `OadrReport`. Copy field definitions from `data-model.md`. Ensure `#[serde(skip_serializing_if = "Option::is_none")]` on `OadrReportBody::eventID` and `OadrReportInterval::intervalPeriod`.

The crate should still compile at this point (no signatures changed yet).

```bash
cargo check -p ven
```

### Step 2 — Update `VtnPort::upsert_report` trait signature

```rust
async fn upsert_report(&self, body: OadrReportBody) -> Result<()>;
```

Now the build breaks at every impl site. Use the compiler errors as your todo list.

### Step 3 — Update `VtnClient::upsert_report` (inherent + trait impl)

In `vtn.rs`:
1. Change the inherent `upsert_report` to accept `OadrReportBody`, serialize to `Value` at the top with `serde_json::to_value(&body)?`, then proceed with existing 409 logic (`&body.reportName` replaces `.get("reportName")`). Return `Ok(())` instead of the parsed response.
2. Update the `VtnPort for VtnClient` impl to delegate to the inherent method.
3. Remove `submit_report` (unused — confirmed by grep).

### Step 4 — Update `MockVtn`

In `services/test_support/mock_vtn.rs`:
1. Change `submitted_reports: Arc<Mutex<Vec<serde_json::Value>>>` → `Arc<Mutex<Vec<OadrReportBody>>>`.
2. Update `submitted()` return type to `Vec<OadrReportBody>`.
3. Update the `upsert_report` trait impl: push `body.clone()`, return `Ok(())`.
4. Update the two internal tests to use struct field access instead of JSON indexing:
   - `mock.submitted()[0]["reportName"]` → `mock.submitted()[0].reportName`

### Step 5 — Update `controller/reporter.rs` public functions

Change return types:
- `build_measurement_report` → `Option<OadrReportBody>`
- `build_measurement_reports_for_active_events` → `Vec<OadrReportBody>`
- `build_measurement_report_for_obligation` → `Option<OadrReportBody>`
- `build_status_report` → `Option<OadrReportBody>`

Replace every `json!({...})` macro call with direct struct construction. Private helpers (`build_soc_intervals`, inline interval construction) return `Vec<OadrReportInterval>`.

Remove `use serde_json::{json, Value};` import once no `json!{}` macros remain.

### Step 6 — Update call sites (three files, type change only)

- `services/obligation.rs`: `if let Some(report) = report_opt` — `report` is now `OadrReportBody`. No other change.
- `tasks/planning.rs`: same pattern.
- `tasks/sim_tick/publish.rs`: `for report in reports` — `report` is now `OadrReportBody`. No other change.

### Step 7 — Update `routes/reports.rs`

```rust
pub async fn post_reports(
    State(ctx): State<AppCtx>,
    Json(body): Json<OadrReportBody>,   // ← was Json<serde_json::Value>
) -> impl IntoResponse {
    match ctx.vtn.upsert_report(body.clone()).await {
        Ok(()) => {
            counter!("reports_sent_total").increment(1);
            (axum::http::StatusCode::CREATED, Json(body)).into_response()
        }
        Err(e) => {
            error!("report submission failed: {e:#}");
            (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": format!("{e:#}")})),
            )
                .into_response()
        }
    }
}
```

`put_report` is unchanged (retains `Json<serde_json::Value>`).

### Step 8 — Update / add tests

In `vtn_port.rs` tests:
- Add `test_oadr_report_body_round_trips_through_json` — constructs an `OadrReportBody`, serializes to `serde_json::Value`, deserializes back, re-serializes, asserts `v1 == v2` (structural equality).
- Add a test covering the `eventID: None` case to confirm absent serialization (not null).

In `reporter.rs` tests:
- Update existing tests to access struct fields by name (e.g., `report.programID`, `report.resources[0].resourceName`).
- Add at least one test per public function verifying a key field directly.

---

## Verify success criteria

```bash
# SC-001: zero Value on public signatures
grep "serde_json::Value" VEN/src/vtn.rs           # only internal helpers (get_json etc.)
grep "serde_json::Value" VEN/src/controller/vtn_port.rs  # only OadrPayload::values, OadrReportPayload::values
grep "serde_json::Value" VEN/src/controller/reporter.rs  # zero on pub fn lines

# SC-002: all tests green
cd VEN && cargo test -p ven
```
