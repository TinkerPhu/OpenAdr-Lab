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

## References

- [Official OpenADR 2.0b Specification](https://www.openadr.org/specification)
- [OpenADR 2.0 Demand Response Program Implementation Guide](https://www.openadr.org/assets/openadr_drprogramguide_v1.0.pdf)
- [OpenADR Schema Repository (GitHub)](https://github.com/sangeeths/OpenADR)
