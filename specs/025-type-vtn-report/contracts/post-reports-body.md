# Contract: POST /reports

**Endpoint**: `POST /reports`  
**Handler**: `VEN/src/routes/reports.rs::post_reports`  
**Change in this feature**: request body typed from raw JSON to `OadrReportBody`; response changes from VTN response body to echo-back of submitted body.

---

## Request

**Content-Type**: `application/json`  
**Body**: `OadrReportBody` (see `data-model.md`)

### Required fields

| Field | Type | Description |
|---|---|---|
| `programID` | string | OpenADR program ID |
| `clientName` | string | VEN name |
| `reportName` | string | Identifies the report for upsert |
| `resources` | array | At least one `OadrReportResource` |

### Optional fields

| Field | Type | Description |
|---|---|---|
| `eventID` | string | Absent for status reports |

### Validation

axum's `Json<OadrReportBody>` extractor enforces required fields. Missing required fields → **422 Unprocessable Entity** (automatic, no handler code needed).

---

## Response

### Success — `201 Created`

Body: the submitted `OadrReportBody` echoed back as JSON (field values unchanged).

```json
{
  "programID": "dr-prog-001",
  "eventID": "evt-abc",
  "clientName": "ven-1",
  "reportName": "auto-ven-1-evt-abc",
  "resources": [ ... ]
}
```

### VTN unreachable — `502 Bad Gateway`

```json
{ "error": "<anyhow error chain>" }
```

### Invalid body — `422 Unprocessable Entity`

Returned automatically by axum when a required field is missing or has the wrong type. No handler code.

---

## Behaviour notes

- The handler forwards the body to the VTN via `VtnPort::upsert_report`. On VTN `409 Conflict`, the VEN client automatically performs an update (upsert semantics) — this is transparent to the caller.
- Unknown JSON fields in the request body are silently ignored (no `deny_unknown_fields`), consistent with all other OpenADR DTOs in this codebase.
- `PUT /reports/{id}` is unchanged — it retains `Json<serde_json::Value>` since it calls `VtnClient::update_report` (not on `VtnPort`).
