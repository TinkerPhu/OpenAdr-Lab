# OpenADR Lab - Frequently Asked Questions

## Program Type Field

### Q: Is `programType` an enum in OpenADR?

**A: No.** The `programType` field is intentionally **free text** in the OpenADR 2.0b specification. Even the official XSD schema does not define an enumeration for it—it's just a string field.

### Q: Why is it free text instead of an enum?

The spec intentionally keeps `programType` flexible because:

1. **Regional variation** — Different regions and regulators use different program type taxonomies. California has different types than NERC regions, for example.
2. **Evolution** — Program types evolve over time, and free text allows VTNs to adapt without waiting for spec updates.
3. **Customization** — It allows VTNs to define their own program categorization scheme.

The OpenADR spec defines the field as: *"A program defined categorization."* This means the categorization is application-specific, not globally mandated.

### Q: What are common `programType` values?

Examples seen in real-world deployments:
- `PRICING_TARIFF` (shown in the official spec example)
- `LOAD_CONTROL`
- `DEMAND_RESPONSE`
- `ANCILLARY_SERVICE`
- `RENEWABLE_INTEGRATION`
- Custom values defined by individual utilities

### Q: Should we add a dropdown for suggested types?

**Optional enhancement:** You could add a combo box (dropdown + free text) that suggests common program types while still allowing custom values. This would improve UX without violating the spec's flexibility principle.

Current implementation: Free text field (matches spec and real-world usage).

---

## Program Description URL

### Q: Why map a single URL field to an array in `programDescriptions`?

The OpenADR spec defines `programDescriptions` as an array of objects (each with a `url` field), but for simplicity in the VTN UI, we expose a single "Description URL" field that maps to the first array entry.

**Mapping:**
- UI form: Single `Description URL` input field
- API data: `programDescriptions: [{ url: "..." }]`

This aligns with the pattern stated in CLAUDE.md: avoid DTO normalization and pass through OpenADR spec field names across all layers, but simplify for UX when reasonable.

---

---

## VEN UI - Sensor Page

### Q: What happens when I submit a sensor reading in the VEN UI?

**A: The sensor reading is stored locally in the VEN's memory only.** It's not sent to the VTN, and it's not a report draft.

### Step-by-step flow:

1. **Fill form** — Enter Temperature (C), Power (W), Voltage (V), and optional Raw JSON
2. **Click Submit** — Makes a `POST /sensors` request to the VEN application
3. **VEN stores locally** — The VEN stores it in-memory with a UUID and timestamp
4. **UI refreshes** — Success message appears, data shows on Sensors page

### Key details:

- **Not sent to VTN** — The VTN never sees this sensor data
- **Not a report** — Reports are submitted separately via the Reports page
- **Local simulation only** — This is mock/test data for the VEN
- **Auto-sampler** — The VEN also generates fake sensor values automatically every 10 seconds
- **Optional persistence** — If configured, sensor state is saved to disk every 15 seconds

**Purpose:** This feature lets you inject test sensor data into the VEN without a real sensor connected. Useful for testing DR event responses and report generation without hardware.

### Q: What should I put in a Report?

**A:** A report contains measurements/telemetry from one or more resources (devices) in response to a DR event. The `resources` JSON field is what you fill in the VEN UI form. Here are 3 realistic examples:

#### Example 1: HVAC Load Reduction Report

An office building's HVAC reduces power demand during a peak demand event.

```json
[
  {
    "resourceName": "HVAC-Building-7",
    "intervalPeriod": { "start": "2026-02-08T14:00:00Z", "duration": "PT15M" },
    "intervals": [
      {
        "id": 0,
        "payloads": [
          { "type": "DEMAND", "values": [52.1] },
          { "type": "BASELINE", "values": [62.0] },
          { "type": "OPERATING_STATE", "values": ["REDUCED"] }
        ]
      },
      {
        "id": 1,
        "payloads": [
          { "type": "DEMAND", "values": [45.2] },
          { "type": "BASELINE", "values": [62.0] },
          { "type": "OPERATING_STATE", "values": ["REDUCED"] }
        ]
      },
      {
        "id": 2,
        "payloads": [
          { "type": "DEMAND", "values": [43.8] },
          { "type": "BASELINE", "values": [62.0] },
          { "type": "OPERATING_STATE", "values": ["REDUCED"] }
        ]
      },
      {
        "id": 3,
        "payloads": [
          { "type": "DEMAND", "values": [55.4] },
          { "type": "BASELINE", "values": [62.0] },
          { "type": "OPERATING_STATE", "values": ["NORMAL"] }
        ]
      }
    ]
  }
]
```

**What it shows:** HVAC normally draws 62 kW (`BASELINE`). Event starts at 14:00 — demand drops from 52 kW to 44 kW over 45 minutes as the system ramps down. At 14:45 (interval 3) the event ends and the HVAC returns to `NORMAL` mode at 55.4 kW, recovering toward baseline. The resource-level `intervalPeriod` defines the timing: interval 0 = 14:00, interval 1 = 14:15, etc. (computed as `start + id * duration`).

---

#### Example 2: EV Charger Managed Charging Report

An EV charger throttles power during a peak pricing event and reports battery state.

```json
[
  {
    "resourceName": "EV-Charger-Bay-3",
    "intervalPeriod": { "start": "2026-02-08T17:00:00Z", "duration": "PT30M" },
    "intervals": [
      {
        "id": 0,
        "payloads": [
          { "type": "DEMAND", "values": [3.3] },
          { "type": "STORAGE_CHARGE_LEVEL", "values": [48] },
          { "type": "STORAGE_MAX_CHARGE_POWER", "values": [7.4] }
        ]
      },
      {
        "id": 1,
        "payloads": [
          { "type": "DEMAND", "values": [3.3] },
          { "type": "STORAGE_CHARGE_LEVEL", "values": [51] },
          { "type": "STORAGE_MAX_CHARGE_POWER", "values": [7.4] }
        ]
      },
      {
        "id": 2,
        "payloads": [
          { "type": "DEMAND", "values": [3.3] },
          { "type": "STORAGE_CHARGE_LEVEL", "values": [53] },
          { "type": "STORAGE_MAX_CHARGE_POWER", "values": [7.4] }
        ]
      },
      {
        "id": 3,
        "payloads": [
          { "type": "DEMAND", "values": [7.4] },
          { "type": "STORAGE_CHARGE_LEVEL", "values": [56] },
          { "type": "STORAGE_MAX_CHARGE_POWER", "values": [7.4] }
        ]
      }
    ]
  }
]
```

**What it shows:** Charger is capable of 7.4 kW (`STORAGE_MAX_CHARGE_POWER`) but throttled to 3.3 kW (`DEMAND`) during peak hours (intervals 0-2). Battery rises slowly: 48% → 51% → 53% at the reduced rate (~2% per 30 min at 3.3 kW on a ~60 kWh battery). At 18:30 (interval 3) the peak event ends and charging resumes at full 7.4 kW, with SOC jumping to 56%.

---

#### Example 3: Aggregated Campus Report

A campus aggregates demand across multiple buildings to prove overall load curtailment.

```json
[
  {
    "resourceName": "AGGREGATED_REPORT",
    "intervalPeriod": { "start": "2026-02-08T14:00:00Z", "duration": "PT15M" },
    "intervals": [
      {
        "id": 0,
        "payloads": [
          { "type": "DEMAND", "values": [420] },
          { "type": "BASELINE", "values": [480] },
          { "type": "LOAD_SHED_DELTA_AVAILABLE", "values": [80] }
        ]
      },
      {
        "id": 1,
        "payloads": [
          { "type": "DEMAND", "values": [340] },
          { "type": "BASELINE", "values": [480] },
          { "type": "LOAD_SHED_DELTA_AVAILABLE", "values": [25] }
        ]
      },
      {
        "id": 2,
        "payloads": [
          { "type": "DEMAND", "values": [305] },
          { "type": "BASELINE", "values": [480] },
          { "type": "LOAD_SHED_DELTA_AVAILABLE", "values": [10] }
        ]
      },
      {
        "id": 3,
        "payloads": [
          { "type": "DEMAND", "values": [310] },
          { "type": "BASELINE", "values": [480] },
          { "type": "LOAD_SHED_DELTA_AVAILABLE", "values": [5] }
        ]
      }
    ]
  }
]
```

**What it shows:** Campus baseline is 480 kW. Over 1 hour the demand ramps down: 420 → 340 → 305 → 310 kW as buildings progressively shed load. Peak reduction of 175 kW (36%) at interval 2. `LOAD_SHED_DELTA_AVAILABLE` drops from 80 to 5 kW, showing the campus is approaching its curtailment limit. `AGGREGATED_REPORT` is a special OpenADR resource name for multi-resource summaries.

---

#### OpenADR 3 Report Payload Types (from openleadr-rs)

These are the standard `type` values for report payloads:

| Type | Purpose | Typical Unit |
|---|---|---|
| `USAGE` | Energy consumed over interval | kWh |
| `DEMAND` | Power draw at a point in time | kW |
| `SETPOINT` | Target value (e.g. thermostat) | kW, °C |
| `BASELINE` | Expected consumption without DR | kW |
| `DELTA_USAGE` | Change in usage vs baseline | kWh |
| `OPERATING_STATE` | Device mode (NORMAL, REDUCED, OFF) | — |
| `READING` | Raw meter reading | kWh, V, A |
| `STORAGE_CHARGE_LEVEL` | Battery state of charge | % |
| `STORAGE_MAX_CHARGE_POWER` | Max charge capability | kW |
| `STORAGE_MAX_DISCHARGE_POWER` | Max discharge capability | kW |
| `STORAGE_USABLE_CAPACITY` | Total usable battery capacity | kWh |
| `LOAD_SHED_DELTA_AVAILABLE` | Additional curtailment possible | kW |
| `GENERATION_DELTA_AVAILABLE` | Additional generation possible | kW |
| `SIMPLE_LEVEL` | Simple 0-3 level indicator | — |
| `DATA_QUALITY` | Confidence in reported data | — |

Custom/private strings are also allowed for application-specific types.

#### Key Points:
- **resources** — Array of devices/systems. Use `"AGGREGATED_REPORT"` for facility-wide summaries.
- **intervalPeriod** — Set once at the **resource level** with `start` and `duration`. Individual interval times are computed as `start + (id × duration)`. Only add `intervalPeriod` on a specific interval if it needs to **override** the resource default (e.g., different duration). Don't repeat it on every interval.
- **payloads** — Array of `{type, values}`. Use standard `ReportType` values from the table above.
- **DEMAND vs BASELINE** — Report both to show the actual reduction. The VTN/utility can calculate savings as `BASELINE - DEMAND`.

---

## References

- [OpenADR 3 Specification](https://www.openadr.org/specification)
- [OpenADR 3.0 Overview](https://www.openadr.org/openadr-3-0)
- [openleadr-rs — OpenADR 3 VTN/VEN in Rust](https://github.com/OpenLEADR/openleadr-rs)
