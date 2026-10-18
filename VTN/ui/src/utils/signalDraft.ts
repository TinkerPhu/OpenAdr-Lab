/**
 * The workbench's draft: the one state every input mode edits.
 *
 * A draft is a pattern of intervals — never an expansion — plus how it repeats.
 * The grid edits it today; the chart and the operators edit it next, through the
 * same functions, so there is no second state to keep in step. Every function
 * here is pure and returns a new draft.
 *
 * What this module deliberately does not know:
 *   - what a signal's unit is or what range is sane — it takes a
 *     `SignalRegistryEntry` for each type it carries;
 *   - when interval *i* of a stored event runs — `draftFromPattern` is handed
 *     intervals the BFF already timed with `lab_core::event_timing`;
 *   - how a recurrence unfolds — the BFF's preview does that with the same rule
 *     the VEN plans against.
 */
import type { EventInput, EventPayloadDescriptor, PatternInterval } from "../api/types";
import {
  durationFromRecurrence,
  formatIsoDuration,
  recurrenceFromDuration,
  type Recurrence,
} from "./recurrence";
import { lookupSignal, type SignalRegistry } from "./signalRegistry";

export interface DraftInterval {
  start: string;
  end: string;
  /** One cell per signal the draft carries; null is an empty or unreadable cell. */
  values: Record<string, number | null>;
}

export interface SignalDraft {
  sourceEventID?: string;
  programID: string;
  eventName: string;
  targets: string[];
  /** The payload types this draft carries, in column order. */
  types: string[];
  intervals: DraftInterval[];
  recurrence: Recurrence;
}

export type { PatternInterval };

export interface SourceEvent {
  id?: string;
  programID: string | null;
  eventName: string | null;
  duration?: string | null;
  priority?: number | null;
  targets?: string[] | null;
}

export type DraftIssueCode =
  | "empty"
  | "value"
  | "range"
  | "period"
  | "overlap"
  | "gap"
  | "recurrence";

export interface DraftIssue {
  code: DraftIssueCode;
  /** Row the issue sits on, when it has one. */
  row?: number;
  /** The signal column it sits in, for a cell issue. */
  type?: string;
  message: string;
}

const ms = (iso: string) => Date.parse(iso);
const hhmmUtc = (iso: string) => `${new Date(iso).toISOString().slice(11, 16)} UTC`;

/** The payload types a stored event's `intervals` carry, once each, in order of appearance. */
export function eventPayloadTypes(intervals: unknown): string[] {
  if (!Array.isArray(intervals)) return [];
  const types: string[] = [];
  for (const iv of intervals) {
    const payloads = (iv as { payloads?: unknown })?.payloads;
    if (!Array.isArray(payloads)) continue;
    for (const p of payloads) {
      const type = (p as { type?: unknown })?.type;
      if (typeof type === "string" && !types.includes(type)) types.push(type);
    }
  }
  return types;
}

export function draftFromPattern(
  event: SourceEvent,
  pattern: PatternInterval[],
  registry: SignalRegistry,
): SignalDraft {
  const types: string[] = [];
  for (const iv of pattern) {
    for (const p of iv.payloads) {
      if (!types.includes(p.type)) {
        lookupSignal(registry, p.type); // refuse a type the editor cannot declare
        types.push(p.type);
      }
    }
  }
  const intervals: DraftInterval[] = pattern.map((iv) => ({
    start: iv.start,
    end: iv.end,
    values: Object.fromEntries(
      types.map((t) => [t, iv.payloads.find((p) => p.type === t)?.value ?? null]),
    ),
  }));
  const recurrence =
    intervals.length === 0
      ? ({ kind: "once" } as const)
      : recurrenceFromDuration(
          event.duration,
          new Date(intervals[0].start),
          new Date(intervals[intervals.length - 1].end),
        );
  return {
    sourceEventID: event.id,
    programID: event.programID ?? "",
    eventName: event.eventName ?? "",
    targets: event.targets ?? [],
    types,
    intervals,
    recurrence,
  };
}

function withRow(draft: SignalDraft, row: number, change: (r: DraftInterval) => DraftInterval) {
  return { ...draft, intervals: draft.intervals.map((r, i) => (i === row ? change(r) : r)) };
}

export function setValue(
  draft: SignalDraft,
  row: number,
  type: string,
  value: number | null,
): SignalDraft {
  return withRow(draft, row, (r) => ({ ...r, values: { ...r.values, [type]: value } }));
}

/** Sets `type` to `value` on rows `fromRow..toRow`, inclusive — fill-down and "set selection". */
export function fillRange(
  draft: SignalDraft,
  type: string,
  fromRow: number,
  toRow: number,
  value: number | null,
): SignalDraft {
  return {
    ...draft,
    intervals: draft.intervals.map((r, i) =>
      i >= fromRow && i <= toRow ? { ...r, values: { ...r.values, [type]: value } } : r,
    ),
  };
}

function parseCell(token: string): number | null {
  const t = token.trim();
  if (t === "") return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

/**
 * Pastes spreadsheet text at (`startRow`, the `startTypeIndex`-th signal).
 *
 * Lines are rows; columns split on a tab, else a comma or semicolon (so a CSV
 * and a spreadsheet selection both work; a decimal comma is not supported, since
 * it cannot be told from a separator). Past the last row the grid is extended
 * with intervals of the last row's length, so a pasted day-ahead series never
 * needs rows to be added first. A token that is not a number becomes an empty
 * cell, which validation then names.
 */
export function pasteRows(
  draft: SignalDraft,
  startRow: number,
  startTypeIndex: number,
  text: string,
): SignalDraft {
  const lines = text.split(/\r?\n/);
  while (lines.length > 0 && lines[lines.length - 1].trim() === "") lines.pop();

  let intervals = draft.intervals;
  for (let n = 0; n < lines.length; n++) {
    const row = startRow + n;
    if (row >= intervals.length) {
      const last = intervals[intervals.length - 1];
      if (!last) break;
      intervals = [
        ...intervals,
        {
          start: last.end,
          end: new Date(ms(last.end) + (ms(last.end) - ms(last.start))).toISOString(),
          values: Object.fromEntries(draft.types.map((t) => [t, null])),
        },
      ];
    }
    const tokens = lines[n].split(lines[n].includes("\t") ? "\t" : /[,;]/);
    const values = { ...intervals[row].values };
    tokens.forEach((token, col) => {
      const type = draft.types[startTypeIndex + col];
      if (type !== undefined) values[type] = parseCell(token);
    });
    intervals = intervals.map((r, i) => (i === row ? { ...r, values } : r));
  }
  return { ...draft, intervals };
}

const REPEATING = new Set(["forever", "span"]);

export function validateDraft(draft: SignalDraft, registry: SignalRegistry): DraftIssue[] {
  const issues: DraftIssue[] = [];
  if (draft.intervals.length === 0) {
    return [{ code: "empty", message: "The series has no intervals." }];
  }
  if (draft.recurrence.kind === "invalid") {
    issues.push({
      code: "recurrence",
      message: `"${draft.recurrence.duration}" is not an ISO 8601 duration.`,
    });
  }
  draft.intervals.forEach((r, row) => {
    for (const type of draft.types) {
      const v = r.values[type];
      const entry = registry[type];
      if (v === null || v === undefined) {
        issues.push({
          code: "value",
          row,
          type,
          message: `Row ${row + 1}: ${type} is empty or not a number.`,
        });
      } else if (entry && (v < entry.min || v > entry.max)) {
        issues.push({
          code: "range",
          row,
          type,
          message: `Row ${row + 1}: ${type} ${v} is outside ${entry.min}..${entry.max} ${entry.unitLabel}.`,
        });
      }
    }
    if (ms(r.end) <= ms(r.start)) {
      issues.push({ code: "period", row, message: `Row ${row + 1} does not end after it starts.` });
    }
    const prev = draft.intervals[row - 1];
    if (prev && ms(r.start) < ms(prev.end)) {
      issues.push({ code: "overlap", row, message: `Row ${row + 1} starts before row ${row} ends.` });
    } else if (prev && ms(r.start) > ms(prev.end) && REPEATING.has(draft.recurrence.kind)) {
      // Receivers shift the whole pattern by (last end - first start): a hole inside
      // it is repeated too, with nothing declared for that time.
      issues.push({
        code: "gap",
        row,
        message: `A repeating series has no price from ${hhmmUtc(prev.end)} to ${hhmmUtc(r.start)}.`,
      });
    }
  });
  return issues;
}

/**
 * The priority a published draft gets so that it wins over the events it
 * replaces. Lower number wins; an absent priority is the lowest of all; at 0 the
 * newer `createdDateTime` breaks the tie (BL-02, `rate_schedule.rs`).
 */
export function supersedePriority(replaced: (number | null | undefined)[]): number {
  const numbers = replaced.filter((p): p is number => typeof p === "number");
  return numbers.length === 0 ? 0 : Math.max(0, Math.min(...numbers) - 1);
}

/**
 * The event to publish. The only way this module builds one, so an event goes out
 * with its contract: every payload type is looked up in the registry (an unknown
 * one throws) and its units and currency are written as `payloadDescriptors`.
 */
export function toEventInput(
  draft: SignalDraft,
  registry: SignalRegistry,
  replacedPriorities: (number | null | undefined)[],
): EventInput {
  const entries = draft.types.map((t) => lookupSignal(registry, t));
  const issues = validateDraft(draft, registry);
  if (issues.length > 0) {
    throw new Error(`draft is invalid: ${issues[0].message}`);
  }
  const payloadDescriptors: EventPayloadDescriptor[] = entries.map((e) => ({
    payloadType: e.payloadType,
    units: e.units,
    ...(e.currency ? { currency: e.currency } : {}),
  }));
  return {
    programID: draft.programID,
    eventName: draft.eventName,
    priority: supersedePriority(replacedPriorities),
    duration: durationFromRecurrence(draft.recurrence),
    intervalPeriod: { start: draft.intervals[0].start },
    targets: draft.targets,
    payloadDescriptors,
    intervals: draft.intervals.map((r, id) => ({
      id,
      intervalPeriod: { start: r.start, duration: formatIsoDuration(ms(r.end) - ms(r.start)) },
      payloads: draft.types.map((type) => ({ type, values: [r.values[type]] })),
    })),
  };
}
