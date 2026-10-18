/**
 * What the signal workbench knows about each signal type it can edit.
 *
 * Two kinds of fact, from two owners:
 *
 *   - what a number on the wire *means* (payload type, unit, currency) is the
 *     wire profile's, and lives in `lab_core::wire_contract`. The BFF serves it
 *     (`GET /api/signals/registry`); this file never restates it, so the editor
 *     cannot declare a unit the VEN reads differently.
 *   - how a person *edits* it (label, step, range, interval grid) is the
 *     editor's, declared below as data.
 *
 * Adding GHG or a capacity envelope is a row in `EDITOR_FACTS` — the editor,
 * validator and publisher take a `SignalRegistryEntry` and never branch on the
 * type.
 */

import type { WireSignalFacts } from "../api/types";

interface EditorFacts {
  /** The legend wording; shared with the tariff chart so a UI string greps to its code. */
  label: string;
  /** Grid step a typed or dragged value snaps to. */
  step: number;
  min: number;
  max: number;
  /** Length of one interval on the editing grid. */
  intervalMinutes: number;
}

export interface SignalRegistryEntry extends EditorFacts, WireSignalFacts {
  /** Shown beside the value, e.g. "EUR/kWh". */
  unitLabel: string;
}

export type SignalRegistry = Record<string, SignalRegistryEntry>;

const PRICE_FACTS = { step: 0.01, min: -1, max: 5, intervalMinutes: 60 };

const EDITOR_FACTS: Record<string, EditorFacts> = {
  PRICE: { label: "Import tariff", ...PRICE_FACTS },
  EXPORT_PRICE: { label: "Export tariff", ...PRICE_FACTS },
};

function unitLabelOf(w: WireSignalFacts): string {
  // The wire's `units` for a price is the denominator ("KWH"), with the currency apart.
  if (w.currency && w.units === "KWH") return `${w.currency}/kWh`;
  return w.units ?? "";
}

/** The entries for every type that is both declared by the wire and editable here. */
export function buildSignalRegistry(wire: WireSignalFacts[]): SignalRegistry {
  const registry: SignalRegistry = {};
  for (const w of wire) {
    const editor = EDITOR_FACTS[w.payloadType];
    if (!editor) continue;
    registry[w.payloadType] = { ...editor, ...w, unitLabel: unitLabelOf(w) };
  }
  return registry;
}

export class UnknownSignalType extends Error {
  constructor(readonly payloadType: string) {
    super(`signal type ${payloadType} is not editable here`);
  }
}

export function lookupSignal(registry: SignalRegistry, payloadType: string): SignalRegistryEntry {
  const entry = registry[payloadType];
  if (!entry) throw new UnknownSignalType(payloadType);
  return entry;
}
