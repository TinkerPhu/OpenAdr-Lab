import type { SignalBand } from "../api/types";

/** Where a band sits on a 0–100% row, given the window it is drawn in. */
export function bandGeometry(band: SignalBand, fromMs: number, toMs: number) {
  const span = Math.max(toMs - fromMs, 1);
  const start = Math.max(Date.parse(band.from), fromMs);
  const end = Math.min(Date.parse(band.to), toMs);
  return {
    leftPct: ((start - fromMs) / span) * 100,
    // Never zero-width: a five-second dispatch in a 24-hour window is still
    // something an operator has to be able to see and hover.
    widthPct: Math.max(((end - start) / span) * 100, 0.4),
  };
}
