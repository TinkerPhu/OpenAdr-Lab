/**
 * Power, in the unit the rest of this lab states it in.
 *
 * One function because every fleet surface answers the same question, and two
 * formatters would eventually round or sign differently -- the divergence
 * `one-concept-one-function` exists to prevent.
 *
 * `null` renders as a dash, never as 0.00 kW: "this VEN has not said" and
 * "this VEN is drawing nothing" are different facts.
 */
export function formatKw(watts: number | null | undefined): string {
  return watts === null || watts === undefined ? "—" : `${(watts / 1000).toFixed(2)} kW`;
}
