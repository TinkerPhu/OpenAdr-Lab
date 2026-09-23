/**
 * Order VEN names the way a person counts, not the way a byte comparison does.
 *
 * `Array.prototype.sort()` is lexicographic, so a twenty-VEN fleet lists as
 * ven-1, ven-10, ven-11 … ven-19, ven-2, ven-20, ven-3. Every reader looking
 * for ven-2 finds it two thirds of the way down, and a legend of twenty
 * entries becomes something to search rather than scan.
 *
 * Compares digit runs numerically and everything else as text, so it needs no
 * knowledge of the `ven-N` shape and keeps working for a `site-3-north` or a
 * renamed fleet. Names with no digits at all fall back to a plain text compare.
 */
const CHUNK = /\d+|\D+/g;

export function compareVenNames(a: string, b: string): number {
  const left = a.match(CHUNK) ?? [a];
  const right = b.match(CHUNK) ?? [b];

  for (let i = 0; i < Math.min(left.length, right.length); i++) {
    const x = left[i];
    const y = right[i];
    const bothNumeric = /^\d/.test(x) && /^\d/.test(y);

    if (bothNumeric) {
      // Number(), not string length: "007" and "7" are the same site.
      const diff = Number(x) - Number(y);
      if (diff !== 0) return diff;
    } else {
      const diff = x.localeCompare(y);
      if (diff !== 0) return diff;
    }
  }
  // One is a prefix of the other ("ven-1" before "ven-1a").
  return left.length - right.length;
}

/** Sort a copy by VEN name — callers hold React state, which must not be sorted in place. */
export function byVenName<T>(items: readonly T[], name: (item: T) => string): T[] {
  return [...items].sort((a, b) => compareVenNames(name(a), name(b)));
}
