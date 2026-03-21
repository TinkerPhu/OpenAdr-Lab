# Research: Reporter Multi-Interval Resampling (RF-05e)

**Date**: 2026-03-21

## Research Questions

### RQ-1: How does the current reporter produce reports and where does it lack multi-interval support?

**Findings**:

The reporter has two independent report paths:

1. **Timer-driven measurement reports** (`main.rs:460-480`): Every `report_interval_s` ticks, `build_measurement_reports_for_active_events()` is called. It iterates active events, extracts the latest snapshot from `AssetHistoryBuffer`, and produces a single-interval report per event. **This path has no awareness of `OadrReportObligation` or interval durations.**

2. **Obligation-driven loop** (`main.rs:496-514`): A separate spawned task polls `due_obligations(now)` every 5 seconds. When an obligation is due, it marks it as fulfilled **but does not build or send a report** — it's a stub (`"obligation fulfilled (stub)"`).

**Decision**: The two paths should be unified. The obligation-driven loop should call the reporter with the obligation's `interval_duration_s` to build a properly resampled multi-interval report. The timer-driven path remains as a fallback for events without reportDescriptors (no obligations).

**Alternatives considered**:
- Keep both paths separate and add resampling only to the timer path → Rejected: the timer path doesn't know the obligation interval, and the obligation path is the correct trigger point for obligation-based reports.
- Merge everything into one loop → Rejected: timer-driven and obligation-driven reports serve different purposes (periodic telemetry vs. spec-mandated reporting).

---

### RQ-2: How to convert AssetHistoryBuffer multi-keyed rows into scalar TimeSeries?

**Findings**:

`AssetHistoryBuffer` stores rows as `{ts, HashMap<String, f64>}`. Each row can have keys like `power_kw`, `soc`, `temperature_c`, etc. The reporter needs a single scalar `TimeSeries` per quantity.

The conversion is straightforward:
- Iterate `to_timeline(window)` results
- For each row, extract the target column (e.g. `"power_kw"`)
- Skip rows where the value is NaN (missing data)
- Collect into `TimeSeries { samples: Vec<(DateTime, f64)>, interpolation }`

**Decision**: Add a helper function `history_to_timeseries(buf: &AssetHistoryBuffer, column: &str, interp: Interpolation, window: Option<...>) -> TimeSeries` in `reporter.rs`. No need for a separate module — it's a 10-line conversion.

**Alternatives considered**:
- Add `to_timeseries()` method on `AssetHistoryBuffer` itself → Rejected: `AssetHistoryBuffer` is in the trace module and shouldn't depend on `TimeSeries` from `common/`. The conversion is reporter-specific.
- Use a trait for the conversion → Rejected: over-engineering for a single call site (Principle IV).

---

### RQ-3: Which interpolation mode for which quantity?

**Findings**:

- **Power** (`power_kw`, net import/export): Step interpolation. Power is a piecewise-constant signal between sim ticks. Time-weighted mean via `resample_uniform()` is correct for aggregation.
- **SoC** (`soc`): The spec says "point-in-time sampling". However, `resample_uniform()` computes time-weighted mean over each bucket, which for a step-interpolated signal is the weighted average — not the point-in-time value at a specific instant.

For SoC, the correct approach is:
- Build a Step-interpolated `TimeSeries` from SoC history
- Use `resample_to_grid()` (not `resample_uniform()`) to sample at interval **end** timestamps → gives the SoC at the end of each interval, which is the point-in-time value.

**Decision**: Use `resample_uniform()` for power quantities, `resample_to_grid()` (at interval end timestamps) for SoC.

**Alternatives considered**:
- Use Linear interpolation for SoC → Rejected: SoC changes in discrete steps (battery charges/discharges in 1s ticks), not linearly. Step + grid sampling at interval ends is more accurate.
- Time-weighted mean for SoC too → Rejected: SoC is a state variable (instantaneous), not a rate variable (accumulated). Averaging over an interval is semantically wrong.

---

### RQ-4: How to handle import/export split per interval?

**Findings**:

Current reporter sums positive `power_kw` across assets for import, negative for export. With resampling, the split must happen **after** aggregation per interval.

Two approaches:
1. Sum all assets' power into a net site `TimeSeries`, resample, then clamp each bucket (positive=import, negative=export).
2. Split positive/negative per asset before resampling, resample each, then sum.

Approach 1 is simpler and correct: the net site power after aggregation gives the actual grid exchange direction per interval.

**Decision**: Sum all assets' `power_kw` into a single net site `TimeSeries`, resample with `resample_uniform(interval)`, then for each bucket: import = max(0, value), export = max(0, -value).

---

### RQ-5: What is the report JSON structure for multiple intervals?

**Findings**:

Current structure:
```json
{
  "resources": [{
    "resourceName": "ven-1-meter",
    "intervals": [{"id": 0, "payloads": [...]}]
  }]
}
```

Multi-interval structure (OpenADR 3.0 §5.3 compliant):
```json
{
  "resources": [{
    "resourceName": "ven-1-meter",
    "intervals": [
      {"id": 0, "payloads": [...]},
      {"id": 1, "payloads": [...]},
      {"id": 2, "payloads": [...]},
      {"id": 3, "payloads": [...]}
    ]
  }]
}
```

Each interval gets a sequential `id` starting from 0. The `intervalPeriod` (start + duration) can optionally be added per interval to make timestamps explicit — this is recommended for clarity.

**Decision**: Emit sequential interval entries with `id` 0..N and add `intervalPeriod` with `start` and `duration` to each interval for traceability.

---

### RQ-6: How to wire obligation fulfillment to actual report building?

**Findings**:

The obligation loop (`main.rs:496-514`) currently:
1. Polls `due_obligations(now)` every 5s
2. Marks each as fulfilled (stub)

Needs to become:
1. Polls `due_obligations(now)` every 5s
2. For each due obligation:
   a. Get asset history from `controller_trace()`
   b. Call `build_measurement_report_for_obligation(obligation, asset_history, ven_name)`
   c. Submit report via `vtn.upsert_report(report)`
   d. Mark obligation fulfilled

The obligation already carries `event_id`, `program_id`, `payload_type`, and `interval_duration_s` — everything the reporter needs.

**Decision**: Extend the obligation loop to call the reporter and submit the result. The timer-driven path remains unchanged as a fallback for events without obligations.
