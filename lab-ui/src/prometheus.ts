/**
 * Prometheus text exposition format, read into rows, for the VEN and VTN Metrics pages.
 *
 * One parser for both UIs: it used to be copied verbatim into each page. Covered by
 * `VEN/ui/src/__tests__/prometheus.test.ts` (shared source is tested from the VEN workspace).
 */

export interface MetricRow {
  name: string;
  labels: Record<string, string>;
  value: number;
}

export function parsePrometheusText(text: string): MetricRow[] {
  const rows: MetricRow[] = [];
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const braceIdx = trimmed.indexOf("{");
    if (braceIdx === -1) {
      const parts = trimmed.split(/\s+/);
      if (parts.length >= 2) {
        rows.push({ name: parts[0], labels: {}, value: parseFloat(parts[1]) });
      }
      continue;
    }
    const name = trimmed.slice(0, braceIdx);
    const closeBrace = trimmed.indexOf("}");
    const labelsStr = trimmed.slice(braceIdx + 1, closeBrace);
    const labels: Record<string, string> = {};
    for (const pair of labelsStr.match(/(\w+)="([^"]*)"/g) ?? []) {
      const eq = pair.indexOf("=");
      labels[pair.slice(0, eq)] = pair.slice(eq + 2, -1);
    }
    const value = parseFloat(trimmed.slice(closeBrace + 1).trim());
    rows.push({ name, labels, value });
  }
  return rows;
}

export function formatLabels(labels: Record<string, string>): string {
  const entries = Object.entries(labels);
  if (entries.length === 0) return "";
  return entries.map(([k, v]) => `${k}="${v}"`).join(", ");
}
