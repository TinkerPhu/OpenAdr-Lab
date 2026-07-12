/** Dynamic VEN discovery for the header dropdown.
 *
 * The dropdown used to be only the hand-seeded trio, so fleet VENs
 * (`fleet.sh up N`) were invisible in the UI. Discovery source is the VTN's
 * own VEN registry, proxied by this UI's nginx at `/api/vens-registry`
 * (→ BFF `GET /api/vens`, which holds the VenManager credential). Discovered
 * names are reached through the dynamic nginx route `/api/dyn/<venName>/`,
 * which resolves the Docker service of the same name — venName == compose
 * service name for both the trio and fleet instances.
 *
 * Names not in DEFAULT_VENS are health-probed first and only reachable ones
 * are offered: `fleet.sh down --purge` removes containers but does NOT
 * deregister VENs from the VTN, so without the probe the dropdown would
 * accumulate dead entries from every past fleet.
 */

export type VenEntry = { label: string; url: string; venName: string };

/** One discovered (non-default) VEN: name + optional WP4.5 persona tag,
 * read from the VEN object's PERSONA attribute set at fleet provisioning. */
export type DiscoveredVen = { venName: string; persona?: string };

// Labels are the venNames so trio and discovered fleet entries read
// consistently in the dropdown (was "VEN1".."VEN3" before discovery existed).
export const DEFAULT_VENS: VenEntry[] = [
  { label: "ven-1", url: import.meta.env.VITE_VEN_1_URL || "/api/ven-1", venName: "ven-1" },
  { label: "ven-2", url: import.meta.env.VITE_VEN_2_URL || "/api/ven-2", venName: "ven-2" },
  { label: "ven-3", url: import.meta.env.VITE_VEN_3_URL || "/api/ven-3", venName: "ven-3" },
];

/** Defaults first (their static nginx routes keep working unchanged), then
 * discovered extras deduped, sorted, and mapped onto the dynamic route.
 * WP4.5: a persona tag shows in the label — `fleet-ven-003 (eco)`. */
export function mergeVens(defaults: VenEntry[], discovered: DiscoveredVen[]): VenEntry[] {
  const known = new Set(defaults.map((v) => v.venName));
  const byName = new Map<string, DiscoveredVen>();
  for (const d of discovered) {
    if (!known.has(d.venName) && !byName.has(d.venName)) byName.set(d.venName, d);
  }
  const extras = [...byName.values()]
    .sort((a, b) => a.venName.localeCompare(b.venName))
    .map(({ venName, persona }) => ({
      label: persona ? `${venName} (${persona})` : venName,
      url: `/api/dyn/${venName}`,
      venName,
    }));
  return [...defaults, ...extras];
}

/** Registered non-default VENs that currently answer `/health`.
 * Throws if the registry endpoint itself fails (react-query surfaces that as
 * a normal query error and the dropdown just stays at the defaults).
 * WP4.5: the persona tag is read from the VEN object's PERSONA attribute
 * (set once at fleet provisioning by gen_fleet_profiles.py). */
export async function fetchDiscoveredVens(
  fetchFn: typeof fetch = fetch,
): Promise<DiscoveredVen[]> {
  const resp = await fetchFn("/api/vens-registry");
  if (!resp.ok) throw new Error(`vens-registry returned ${resp.status}`);
  const vens = (await resp.json()) as Array<{
    venName?: string;
    attributes?: Array<{ type?: string; values?: unknown[] }> | null;
  }>;

  const known = new Set(DEFAULT_VENS.map((v) => v.venName));
  const candidates = vens
    .filter(
      (v): v is { venName: string; attributes?: Array<{ type?: string; values?: unknown[] }> | null } =>
        typeof v.venName === "string" && v.venName.length > 0 && !known.has(v.venName),
    )
    .map((v) => {
      const personaValue = (v.attributes ?? []).find((a) => a.type === "PERSONA")?.values?.[0];
      return {
        venName: v.venName,
        persona: typeof personaValue === "string" ? personaValue : undefined,
      };
    });

  const probes = await Promise.all(
    candidates.map(async (ven) => {
      try {
        const r = await fetchFn(`/api/dyn/${ven.venName}/health`);
        return r.ok ? ven : null;
      } catch {
        return null; // unreachable (e.g. purged fleet container) — hide it
      }
    }),
  );
  return probes.filter((v): v is DiscoveredVen => v !== null);
}
