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
 *
 * BL-41: a VEN on a different physical host (not reachable via this UI's
 * Docker DNS) can carry a DASHBOARD_URL attribute — its own full origin,
 * e.g. `http://192.168.1.104:8211`. When present, that origin is used
 * directly (browser fetches the VEN's API straight, CORS is open on the VEN
 * side) instead of the same-host `/api/dyn/<venName>` route.
 */

export type VenEntry = { label: string; url: string; venName: string };

/** One discovered (non-default) VEN: name + optional WP4.5 persona tag,
 * read from the VEN object's PERSONA attribute set at fleet provisioning.
 * BL-41: optional dashboardUrl, read from the DASHBOARD_URL attribute, lets
 * a VEN on a different physical host advertise its own reachable origin
 * instead of relying on same-host Docker DNS. */
export type DiscoveredVen = { venName: string; persona?: string; dashboardUrl?: string };

// Labels are the venNames so trio and discovered fleet entries read
// consistently in the dropdown (was "VEN1".."VEN3" before discovery existed).
export const DEFAULT_VENS: VenEntry[] = [
  { label: "ven-1", url: import.meta.env.VITE_VEN_1_URL || "/api/ven-1", venName: "ven-1" },
  { label: "ven-2", url: import.meta.env.VITE_VEN_2_URL || "/api/ven-2", venName: "ven-2" },
  { label: "ven-3", url: import.meta.env.VITE_VEN_3_URL || "/api/ven-3", venName: "ven-3" },
];

/** Natural-order string comparator: numeric runs compare by value, not digit
 * by digit, so "ven-4" sorts before "ven-10" (plain string/localeCompare
 * sort puts "ven-10".."ven-13" before "ven-4".."ven-9" instead). */
function naturalCompare(a: string, b: string): number {
  const split = (s: string) => s.match(/\d+|\D+/g) ?? [];
  const aParts = split(a);
  const bParts = split(b);
  const len = Math.max(aParts.length, bParts.length);
  for (let i = 0; i < len; i++) {
    const ap = aParts[i] ?? "";
    const bp = bParts[i] ?? "";
    if (ap === bp) continue;
    const an = Number(ap);
    const bn = Number(bp);
    if (!Number.isNaN(an) && !Number.isNaN(bn) && ap !== "" && bp !== "") return an - bn;
    return ap < bp ? -1 : 1;
  }
  return 0;
}

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
    .sort((a, b) => naturalCompare(a.venName, b.venName))
    .map(({ venName, persona, dashboardUrl }) => ({
      label: persona ? `${venName} (${persona})` : venName,
      url: dashboardUrl ?? `/api/dyn/${venName}`,
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
    .map((v): DiscoveredVen => {
      const personaValue = (v.attributes ?? []).find((a) => a.type === "PERSONA")?.values?.[0];
      const dashboardUrlValue = (v.attributes ?? []).find((a) => a.type === "DASHBOARD_URL")
        ?.values?.[0];
      return {
        venName: v.venName,
        persona: typeof personaValue === "string" ? personaValue : undefined,
        dashboardUrl: typeof dashboardUrlValue === "string" ? dashboardUrlValue : undefined,
      };
    });

  const probes = await Promise.all(
    candidates.map(async (ven) => {
      try {
        const base = ven.dashboardUrl ?? `/api/dyn/${ven.venName}`;
        const r = await fetchFn(`${base}/health`);
        return r.ok ? ven : null;
      } catch {
        return null; // unreachable (e.g. purged fleet container) — hide it
      }
    }),
  );
  return probes.filter((v): v is DiscoveredVen => v !== null);
}
