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

export const DEFAULT_VENS: VenEntry[] = [
  { label: "VEN1", url: import.meta.env.VITE_VEN_1_URL || "/api/ven-1", venName: "ven-1" },
  { label: "VEN2", url: import.meta.env.VITE_VEN_2_URL || "/api/ven-2", venName: "ven-2" },
  { label: "VEN3", url: import.meta.env.VITE_VEN_3_URL || "/api/ven-3", venName: "ven-3" },
];

/** Defaults first (their static nginx routes keep working unchanged), then
 * discovered extras deduped, sorted, and mapped onto the dynamic route. */
export function mergeVens(defaults: VenEntry[], discoveredNames: string[]): VenEntry[] {
  const known = new Set(defaults.map((v) => v.venName));
  const extras = [...new Set(discoveredNames)]
    .filter((name) => !known.has(name))
    .sort()
    .map((venName) => ({ label: venName, url: `/api/dyn/${venName}`, venName }));
  return [...defaults, ...extras];
}

/** Registered non-default venNames that currently answer `/health`.
 * Throws if the registry endpoint itself fails (react-query surfaces that as
 * a normal query error and the dropdown just stays at the defaults). */
export async function fetchDiscoveredVens(fetchFn: typeof fetch = fetch): Promise<string[]> {
  const resp = await fetchFn("/api/vens-registry");
  if (!resp.ok) throw new Error(`vens-registry returned ${resp.status}`);
  const vens = (await resp.json()) as Array<{ venName?: string }>;

  const known = new Set(DEFAULT_VENS.map((v) => v.venName));
  const candidates = vens
    .map((v) => v.venName)
    .filter((n): n is string => typeof n === "string" && n.length > 0 && !known.has(n));

  const probes = await Promise.all(
    candidates.map(async (name) => {
      try {
        const r = await fetchFn(`/api/dyn/${name}/health`);
        return r.ok ? name : null;
      } catch {
        return null; // unreachable (e.g. purged fleet container) — hide it
      }
    }),
  );
  return probes.filter((n): n is string => n !== null);
}
