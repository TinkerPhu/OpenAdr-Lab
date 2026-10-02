//! The pre-filled example report body the Reports page offers for a selected
//! event — a pure transform, extracted from `pages/Reports.tsx` so that file
//! exports only components (`react-refresh/only-export-components`).

import type { VtnEvent } from "../api/types";

export function buildExampleResources(event: VtnEvent, venName: string): string {
  const intervals = (event.intervals ?? []).map((iv) => ({
    id: iv.id,
    payloads: (iv.payloads ?? []).map((p) => ({
      type: p.type,
      values: p.values.map((v) => {
        if (p.type === "SIMPLE" && v === 0) return 1;
        if (v === 0) return 0;
        const offset = 1 + (Math.random() * 0.08 - 0.04); // ±4%
        return Math.round(v * offset * 10) / 10;
      }),
    })),
  }));
  const resource = { resourceName: `${venName}-meter`, intervals };
  return JSON.stringify([resource], null, 2);
}
