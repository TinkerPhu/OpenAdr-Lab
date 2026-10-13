import type { CSSProperties } from "react";

/** Shared tooltip container styling for the declarative `<Tooltip contentStyle=... />`
 * prop path (recharts supplies the box chrome — background/border/radius — itself; these
 * three just override the compact font/padding used across every chart). */
export const TOOLTIP_CONTENT_STYLE: CSSProperties = { fontSize: 9, padding: "1px 5px" };
export const TOOLTIP_ITEM_STYLE: CSSProperties = { padding: "0" };
export const TOOLTIP_LABEL_STYLE: CSSProperties = { fontSize: 9, marginBottom: 1 };

/** For custom (non-declarative) tooltip content components that can't use the
 * `<Tooltip contentStyle=... />` prop path (e.g. because they must aggregate multiple
 * series into one row before rendering, as `StackedAreaTooltip` does) — replicates
 * recharts' own default tooltip box chrome so both tooltip styles look identical to the
 * user despite being built through two different mechanisms. */
export const TOOLTIP_BOX_STYLE: CSSProperties = {
  background: "rgba(255,255,255,0.95)",
  border: "1px solid #ccc",
  borderRadius: 4,
  padding: "1px 5px",
  fontSize: 9,
};
