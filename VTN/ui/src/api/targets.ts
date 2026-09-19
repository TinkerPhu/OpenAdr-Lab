import type { Targets } from "./types";

/**
 * Who an object is addressed to.
 *
 * OpenADR 3.1 targets are a flat list of strings. A VEN sees an object when its
 * own targets (plus its resources') intersect the object's, and an empty list
 * means "everyone" — so "is this open?" and "does it reach this VEN?" are the
 * two questions worth asking, and they are asked from several pages.
 *
 * They lived as three separate `.filter((t) => t.type === "VEN_NAME")` chains
 * before the 3.1 migration. Reading a plain array back is trivial enough that
 * three copies would drift unnoticed rather than break loudly, which is exactly
 * the case for keeping one.
 *
 * The lab convention that a VEN's target equals its `venName` is what lets a
 * program address `"ven-1"`; it is a convention, not a protocol rule.
 */

type Targetable = { targets?: Targets | null };

/** The target strings on an object, never null. */
export function targetsOf(obj: Targetable): string[] {
  return obj.targets ?? [];
}

/** True when an object carries no targets, i.e. every VEN can see it. */
export function isOpenToAll(obj: Targetable): boolean {
  return targetsOf(obj).length === 0;
}

/** True when an object explicitly addresses this VEN. Open objects are not "targeted". */
export function targets(obj: Targetable, venName: string): boolean {
  return targetsOf(obj).includes(venName);
}

/** Human-readable audience, for a list cell or chip row. */
export function audienceLabel(obj: Targetable): string {
  return isOpenToAll(obj) ? "Open — all VENs" : targetsOf(obj).join(", ");
}
