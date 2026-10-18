/**
 * Edits that shape a whole series at once: scale it, shift it, cap it, paint a range, or start
 * from a template.
 *
 * Each operator is declared once, as data, in `SIGNAL_OPERATORS`; the toolbar renders from the
 * table and `applyOperator` is the only way to run one. Every operator is a pure
 * `SignalDraft -> SignalDraft` over one signal column: it never touches the times, the other
 * columns or the recurrence, so what a recurring series repeats by stays what the user set.
 *
 * Hours are UTC hours of the row's start, the same clock the grid shows.
 */
import type { DraftInterval, SignalDraft } from "./signalDraft";
import { lookupSignal, type SignalRegistry } from "./signalRegistry";

const HOUR_MS = 3_600_000;

export interface OperatorParam {
  key: string;
  label: string;
  default: number;
  /** Input granularity in the toolbar; omitted means free. */
  step?: number;
}

type Params = Record<string, number>;
type Cell = number | null;

export interface SignalOperator {
  label: string;
  params: OperatorParam[];
  /** The new column, one value per row, given the old one and the registry step. */
  column(draft: SignalDraft, type: string, p: Params): Cell[];
}

/** `value` on the registry's grid, free of float noise (0.1 * 1.2 is not 0.12 in binary). */
function roundToStep(value: number, step: number): number {
  const decimals = (String(step).split(".")[1] ?? "").length;
  return Number((Math.round(value / step) * step).toFixed(decimals));
}

const hourOfDay = (row: DraftInterval) => {
  const d = new Date(row.start);
  return d.getUTCHours() + d.getUTCMinutes() / 60;
};

/** `[from, to)`, wrapping past midnight when `from > to` (22 -> 6 is the night). */
const inHours = (h: number, from: number, to: number) =>
  from <= to ? h >= from && h < to : h >= from || h < to;

const isRepeating = (d: SignalDraft) =>
  d.recurrence.kind === "forever" || d.recurrence.kind === "span";

const column = (d: SignalDraft, type: string) => d.intervals.map((r) => r.values[type]);

const hourParams = (from: string, to: string, fromDefault: number, toDefault: number): OperatorParam[] => [
  { key: from, label: `${from} (UTC h)`, default: fromDefault, step: 1 },
  { key: to, label: `${to} (UTC h)`, default: toDefault, step: 1 },
];

export const SIGNAL_OPERATORS: Record<string, SignalOperator> = {
  scale: {
    label: "Scale by a factor",
    params: [{ key: "factor", label: "factor", default: 1.2, step: 0.05 }],
    column: (d, type, p) => column(d, type).map((v) => (v === null ? null : v * p.factor)),
  },
  shift: {
    label: "Shift later by hours",
    params: [{ key: "hours", label: "hours (negative = earlier)", default: 1, step: 1 }],
    column: (d, type, p) => {
      const values = column(d, type);
      const n = values.length;
      const first = d.intervals[0];
      if (!first) return values;
      const rowMs = Date.parse(first.end) - Date.parse(first.start);
      const k = Math.round((p.hours * HOUR_MS) / rowMs);
      // A repeating series wraps inside its period, so the pattern stays contiguous; a series that
      // does not repeat has nothing to wrap into, so its edge value is held instead.
      return values.map((_, i) =>
        isRepeating(d) ? values[(((i - k) % n) + n) % n] : values[Math.min(n - 1, Math.max(0, i - k))],
      );
    },
  },
  cap: {
    label: "Cap at a maximum",
    params: [{ key: "max", label: "max", default: 0.3, step: 0.01 }],
    column: (d, type, p) => column(d, type).map((v) => (v === null ? null : Math.min(v, p.max))),
  },
  addRange: {
    label: "Add to a range of hours",
    params: [...hourParams("from", "to", 17, 20), { key: "delta", label: "add", default: 0.2, step: 0.01 }],
    column: (d, type, p) =>
      d.intervals.map((r) => {
        const v = r.values[type];
        return v !== null && inHours(hourOfDay(r), p.from, p.to) ? v + p.delta : v;
      }),
  },
  flat: {
    label: "Template: flat",
    params: [{ key: "value", label: "value", default: 0.25, step: 0.01 }],
    column: (d, _type, p) => d.intervals.map(() => p.value),
  },
  twoTier: {
    label: "Template: two tiers",
    params: [
      { key: "offPeak", label: "off-peak", default: 0.1, step: 0.01 },
      { key: "peak", label: "peak", default: 0.4, step: 0.01 },
      ...hourParams("peakFrom", "peakTo", 17, 20),
    ],
    column: (d, _type, p) =>
      d.intervals.map((r) => (inHours(hourOfDay(r), p.peakFrom, p.peakTo) ? p.peak : p.offPeak)),
  },
  threeTier: {
    label: "Template: three tiers",
    params: [
      { key: "offPeak", label: "off-peak", default: 0.1, step: 0.01 },
      { key: "shoulder", label: "shoulder", default: 0.2, step: 0.01 },
      { key: "peak", label: "peak", default: 0.4, step: 0.01 },
      ...hourParams("shoulderFrom", "shoulderTo", 7, 22),
      ...hourParams("peakFrom", "peakTo", 17, 20),
    ],
    column: (d, _type, p) =>
      d.intervals.map((r) => {
        const h = hourOfDay(r);
        if (inHours(h, p.peakFrom, p.peakTo)) return p.peak;
        return inHours(h, p.shoulderFrom, p.shoulderTo) ? p.shoulder : p.offPeak;
      }),
  },
};

/** Runs operator `id` over one signal column; a missing parameter takes the declared default. */
export function applyOperator(
  id: string,
  draft: SignalDraft,
  registry: SignalRegistry,
  type: string,
  params: Params,
): SignalDraft {
  const op = SIGNAL_OPERATORS[id];
  if (!op) throw new Error(`unknown operator ${id}`);
  const { step } = lookupSignal(registry, type);
  const p: Params = Object.fromEntries(op.params.map((def) => [def.key, params[def.key] ?? def.default]));
  const values = op.column(draft, type, p);
  return {
    ...draft,
    intervals: draft.intervals.map((r, i) => ({
      ...r,
      values: { ...r.values, [type]: values[i] === null ? null : roundToStep(values[i] as number, step) },
    })),
  };
}
