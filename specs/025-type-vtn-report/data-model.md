# Data Model: Type the VTN Report Interface

**Feature**: `025-type-vtn-report`  
**Phase**: 1 — Design  
**Date**: 2026-05-14  
**Module**: `VEN/src/controller/vtn_port.rs`

---

## New Structs

All four structs derive `Debug`, `Clone`, `Serialize`, `Deserialize`. All field names use OpenADR 3 camelCase verbatim (module-level `#![allow(non_snake_case)]` already present in `vtn_port.rs`).

### `OadrReportBody`

Top-level envelope for a report submission to the VTN.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportBody {
    pub programID: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eventID: Option<String>,
    pub clientName: String,
    pub reportName: String,
    pub resources: Vec<OadrReportResource>,
}
```

| Field | Type | Required | Wire name | Notes |
|---|---|---|---|---|
| `programID` | `String` | Yes | `"programID"` | OpenADR program identifier |
| `eventID` | `Option<String>` | No | `"eventID"` | Absent (not null) when None — status reports omit it |
| `clientName` | `String` | Yes | `"clientName"` | VEN name |
| `reportName` | `String` | Yes | `"reportName"` | Identifies the report for upsert matching |
| `resources` | `Vec<OadrReportResource>` | Yes | `"resources"` | One entry per resource/asset |

---

### `OadrReportResource`

A named resource (site meter, individual asset) within a report.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportResource {
    pub resourceName: String,
    pub intervals: Vec<OadrReportInterval>,
}
```

| Field | Type | Required | Wire name | Notes |
|---|---|---|---|---|
| `resourceName` | `String` | Yes | `"resourceName"` | e.g., `"ven-1-meter"`, `"ven-1-ev"` |
| `intervals` | `Vec<OadrReportInterval>` | Yes | `"intervals"` | One or more measurement intervals |

---

### `OadrReportInterval`

A single measurement interval with an optional time window and one or more payload values.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportInterval {
    pub id: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intervalPeriod: Option<OadrIntervalPeriod>,
    pub payloads: Vec<OadrReportPayload>,
}
```

| Field | Type | Required | Wire name | Notes |
|---|---|---|---|---|
| `id` | `usize` | Yes | `"id"` | Zero-based sequential index within the resource |
| `intervalPeriod` | `Option<OadrIntervalPeriod>` | No | `"intervalPeriod"` | Absent on single-interval (timer-driven) reports; present on multi-interval obligation reports |
| `payloads` | `Vec<OadrReportPayload>` | Yes | `"payloads"` | One entry per payload type in the interval |

**Reuses existing type**: `OadrIntervalPeriod` (already defined in `vtn_port.rs`) — fields `start: Option<String>` (ISO 8601 datetime) and `duration: Option<String>` (ISO 8601 duration).

---

### `OadrReportPayload`

A single typed value (or set of values) within an interval.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OadrReportPayload {
    pub r#type: String,
    pub values: Vec<serde_json::Value>,
}
```

| Field | Type | Required | Wire name | Notes |
|---|---|---|---|---|
| `r#type` | `String` | Yes | `"type"` | e.g., `"USAGE"`, `"OPERATING_STATE"`, `"STORAGE_CHARGE_LEVEL"`, `"TELEMETRY_STATUS"`, `"SIMPLE"` |
| `values` | `Vec<serde_json::Value>` | Yes | `"values"` | Heterogeneous — numbers for power, strings for state/SoC. See note below. |

**Note on `values`**: The OpenADR 3 spec defines `values` as a polymorphic array. Current payload types produce:
- `[1234.5]` — f64, USAGE (watts)
- `["ACTIVE"]` — String, OPERATING_STATE
- `["82.3"]` — String, STORAGE_CHARGE_LEVEL (percentage as string)
- `["PlanCycle trigger=..."]` — String, TELEMETRY_STATUS
- `[1.0]` — f64, SIMPLE

`Vec<serde_json::Value>` is a deliberate choice (same as `OadrPayload::values`), not a gap.

---

## Updated Trait Signature

```rust
#[async_trait]
pub trait VtnPort: Send + Sync {
    async fn fetch_programs(&self) -> Result<Vec<OadrProgram>>;
    async fn fetch_events(&self) -> Result<Vec<OadrEvent>>;
    async fn fetch_reports(&self) -> Result<Vec<OadrReport>>;
    async fn upsert_report(&self, body: OadrReportBody) -> Result<()>;  // ← changed
}
```

---

## Updated `MockVtn` Storage

```rust
pub struct MockVtn {
    // ...
    pub submitted_reports: Arc<Mutex<Vec<OadrReportBody>>>,  // ← was Vec<serde_json::Value>
    // ...
}

impl MockVtn {
    pub fn submitted(&self) -> Vec<OadrReportBody> {         // ← was Vec<serde_json::Value>
        self.submitted_reports.lock().unwrap().clone()
    }
}
```

---

## Wire Format Example

Measurement report (single interval, timer-driven):

```json
{
  "programID": "dr-prog-001",
  "eventID": "evt-abc",
  "clientName": "ven-1",
  "reportName": "auto-ven-1-evt-abc",
  "resources": [
    {
      "resourceName": "ven-1-meter",
      "intervals": [
        {
          "id": 0,
          "payloads": [
            { "type": "USAGE", "values": [4500.0] },
            { "type": "OPERATING_STATE", "values": ["ACTIVE"] },
            { "type": "STORAGE_CHARGE_LEVEL", "values": ["72.4"] }
          ]
        }
      ]
    }
  ]
}
```

Status report (no `eventID`, no `intervalPeriod`):

```json
{
  "programID": "dr-prog-001",
  "clientName": "ven-1",
  "reportName": "status-ven-1",
  "resources": [
    {
      "resourceName": "ven-1-site",
      "intervals": [
        {
          "id": 0,
          "payloads": [
            { "type": "TELEMETRY_STATUS", "values": ["PlanCycle trigger=Periodic slots=288"] },
            { "type": "USAGE", "values": [2100.0] }
          ]
        }
      ]
    }
  ]
}
```

Obligation report (multi-interval with `intervalPeriod`):

```json
{
  "programID": "dr-prog-001",
  "eventID": "evt-abc",
  "clientName": "ven-1",
  "reportName": "ob-ven-1-evt-abc-USAGE",
  "resources": [
    {
      "resourceName": "ven-1-meter",
      "intervals": [
        {
          "id": 0,
          "intervalPeriod": { "start": "2026-01-01T10:00:00Z", "duration": "PT15M" },
          "payloads": [
            { "type": "USAGE", "values": [3200.0] },
            { "type": "OPERATING_STATE", "values": ["ACTIVE"] }
          ]
        },
        {
          "id": 1,
          "intervalPeriod": { "start": "2026-01-01T10:15:00Z", "duration": "PT15M" },
          "payloads": [
            { "type": "USAGE", "values": [2800.0] },
            { "type": "OPERATING_STATE", "values": ["ACTIVE"] }
          ]
        }
      ]
    }
  ]
}
```
