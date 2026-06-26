/** Shared layout constants for all controller row cell charts.
 * Single source of truth — import here instead of hardcoding in each chart component.
 */
export const CELL_CHART_HEIGHT = 140; // px
export const CELL_CHART_HEIGHT_TALL = Math.round(CELL_CHART_HEIGHT * 2.5); // 350 px
export const CELL_CHART_MIN_WIDTH = 200; // px
/** Fixed width of the left info panel in every cell row (asset, tariff, accumulated).
 * Keeping this identical across all rows aligns the chart left edges (y-axes). */
export const CELL_LEFT_SECTION_WIDTH = 196; // px

/** Default time window: 1 h back, 1 h forward from now. */
export const DEFAULT_WINDOW = { hoursBack: 1.0, hoursForward: 1.0 };
/** Extended time window: 1 h back, 48 h forward (full plan horizon). */
export const EXTENDED_WINDOW = { hoursBack: 1.0, hoursForward: 48.0 };
