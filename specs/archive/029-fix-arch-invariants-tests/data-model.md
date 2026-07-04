# Data Model: Fix Architecture Invariant Gaps and Missing Tests (029)

No new persistent entities or storage schema changes are introduced.

## Existing Type: AssetReportSample

**Location**: `VEN/src/controller/reporter.rs`  
**Already exists**: yes — exported since feature 026.

```
AssetReportSample
  ts         : DateTime<Utc>     — measurement timestamp
  power_kw   : f64               — net power (positive = import)
  soc        : Option<f64>       — state-of-charge 0.0–1.0; None for non-storage assets
```

**Role in this feature**: The obligation service's `check_and_report` signature changes from accepting `Arc<Mutex<SimState>>` to accepting `HashMap<String, Vec<AssetReportSample>>`. This type is the boundary-crossing container — it carries simulator-originated data without importing any simulator type.

## Signature Change: ObligationService::check_and_report

**Before**:
```
check_and_report(state, sim: &Arc<Mutex<SimState>>, vtn, ven_name, now) -> Result<()>
```

**After**:
```
check_and_report(state, asset_samples: HashMap<String, Vec<AssetReportSample>>, vtn, ven_name, now) -> Result<()>
```

The lock acquisition and `(ts, power_kw, soc)` mapping moves to `tasks/obligation.rs`.

## No other data model changes

Items 3, 4, 5 produce test code, a Default impl, and a doc fix — no entity or API schema changes.
