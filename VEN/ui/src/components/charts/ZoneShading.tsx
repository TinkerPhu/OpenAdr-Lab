import { ReferenceArea } from "recharts";
import type { ZoneDef } from "../../api/types";

/**
 * Zone background shading, shared by every time-series chart. Returns an array of
 * elements directly (not a wrapping component) — see NowLine.tsx's doc comment for why:
 * recharts must see actual `<ReferenceArea>` elements as direct children of
 * `<ComposedChart>`, not an intermediate custom component type.
 */
export function renderZoneShading(yAxisId: string, zones: ZoneDef[] | undefined) {
  return zones?.map((z, i) => (
    <ReferenceArea
      key={z.from}
      yAxisId={yAxisId}
      x1={new Date(z.from).getTime()}
      x2={new Date(z.to).getTime()}
      fill={`rgba(0,0,0,${0.04 * (i + 1)})`}
      ifOverflow="hidden"
    />
  ));
}
