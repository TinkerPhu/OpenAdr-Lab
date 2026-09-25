/**
 * Where the lab is, for anything that needs to know about the sun.
 *
 * The authoritative copy is `weather_pv.latitude_deg` / `.longitude_deg` in the
 * VEN profiles (`VEN/profiles/ven-*.yaml` — all fourteen that carry it agree),
 * which is what the VEN's own PV forecast computes against. That is server-side
 * YAML: it never reaches a browser, and no API exposes it, so the VTN UI cannot
 * read it and this is a second home for the value.
 *
 * Recorded in docs/reference/TECHNICAL_DEBTS.md with the TypeScript solar port
 * it exists for. If the lab ever moves, or a second site appears, the fix is to
 * serve the location from the BFF rather than to edit this in step.
 */
export const LAB_LOCATION = {
  latitudeDeg: 47.4491,
  longitudeDeg: 7.8081,
} as const;
