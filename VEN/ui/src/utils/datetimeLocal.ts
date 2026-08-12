// Shared local <-> ISO 8601 conversion for `datetime-local` inputs. Extracted
// from near-identical copies in EvCard/HeaterCard/ShiftableLoadsCard/
// BaselineOverrideCard (generic-over-bespoke: one shared primitive instead of
// four near-identical helpers).

/** Date -> "YYYY-MM-DDTHH:mm" local value for a datetime-local input. */
export function dateToLocalInputValue(d: Date): string {
  const off = d.getTimezoneOffset();
  const local = new Date(d.getTime() - off * 60_000);
  return local.toISOString().slice(0, 16);
}

/** ISO 8601 (wire format) -> "YYYY-MM-DDTHH:mm" local value for a datetime-local input. */
export function isoToLocalInput(iso: string): string {
  return dateToLocalInputValue(new Date(iso));
}

/**
 * datetime-local input value -> ISO 8601 (wire format), or `null` if `local`
 * isn't (yet) a valid date — e.g. the field was cleared or is mid-edit.
 * Callers should skip the update on `null` rather than propagate an invalid
 * value, so a momentarily-empty input degrades gracefully instead of
 * throwing (new Date(local).toISOString() throws RangeError on an invalid date).
 */
export function localInputToIso(local: string): string | null {
  const d = new Date(local);
  return Number.isNaN(d.getTime()) ? null : d.toISOString();
}
