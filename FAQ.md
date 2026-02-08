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

**A:** A report contains measurements/telemetry from one or more resources (devices) in response to a DR event. Here are 3 realistic examples:

#### Example 1: HVAC Temperature Response Report
```json
{
  "resources": [
    {
      "resourceName": "HVAC-Unit-1",
      "intervals": [
        {
          "id": 0,
          "payloads": [
            {
              "type": "TEMPERATURE",
              "values": [22.5]
            },
            {
              "type": "MODE",
              "values": ["HEATING"]
            }
          ]
        },
        {
          "id": 1,
          "payloads": [
            {
              "type": "TEMPERATURE",
              "values": [23.1]
            },
            {
              "type": "MODE",
              "values": ["HEATING"]
            }
          ]
        }
      ]
    }
  ]
}
```
**Context:** VTN sent a heating reduction event. HVAC reports temperature readings every 5 minutes and current operating mode.

---

#### Example 2: EV Charger State Report
```json
{
  "resources": [
    {
      "resourceName": "EV_CHARGER_ZONE_A",
      "intervals": [
        {
          "id": 0,
          "payloads": [
            {
              "type": "POWER_REAL",
              "values": [7500]
            },
            {
              "type": "STATE_OF_CHARGE",
              "values": [45]
            }
          ]
        },
        {
          "id": 1,
          "payloads": [
            {
              "type": "POWER_REAL",
              "values": [3700]
            },
            {
              "type": "STATE_OF_CHARGE",
              "values": [62]
            }
          ]
        }
      ]
    }
  ]
}
```
**Context:** VTN sent a managed charging event during peak hours. EV charger reduced from 7.5kW to 3.7kW and reports actual power draw and vehicle battery state of charge.

---

#### Example 3: Aggregated Multi-Resource Report
```json
{
  "resources": [
    {
      "resourceName": "AGGREGATED_REPORT",
      "intervals": [
        {
          "id": 0,
          "payloads": [
            {
              "type": "DEMAND_RESPONSE_BASELINE",
              "values": [15000]
            },
            {
              "type": "ACTUAL_POWER",
              "values": [12500]
            },
            {
              "type": "COMPLIANCE_STATUS",
              "values": ["COMPLIANT"]
            }
          ]
        },
        {
          "id": 1,
          "payloads": [
            {
              "type": "DEMAND_RESPONSE_BASELINE",
              "values": [15000]
            },
            {
              "type": "ACTUAL_POWER",
              "values": [13200]
            },
            {
              "type": "COMPLIANCE_STATUS",
              "values": ["COMPLIANT"]
            }
          ]
        }
      ]
    }
  ]
}
```
**Context:** Aggregated report across 5 facilities. VTN requested load curtailment. Report shows combined baseline (15kW expected), actual usage (12.5-13.2kW), and compliance.

---

#### Key Points:
- **resources** — Array of devices/systems reporting data. Use `"AGGREGATED_REPORT"` for facility-wide summaries.
- **intervals** — Time periods (usually 5-15 min) with measurements. Each interval has an `id` (0, 1, 2...) and `payloads`.
- **payloads** — Array of measurement types. Each has a `type` (POWER_REAL, TEMPERATURE, STATE_OF_CHARGE, etc.) and `values` array.
- **values** — The actual readings. Can be numbers, strings, or booleans depending on type.

---

## References

- [Official OpenADR 2.0b Specification](https://www.openadr.org/specification)
- [OpenADR 2.0 Demand Response Program Implementation Guide](https://www.openadr.org/assets/openadr_drprogramguide_v1.0.pdf)
- [OpenADR Schema Repository (GitHub)](https://github.com/sangeeths/OpenADR)
