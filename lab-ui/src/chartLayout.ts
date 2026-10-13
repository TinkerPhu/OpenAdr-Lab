/** Shared layout constants for chart components across the app.
 * Single source of truth — import here instead of hardcoding in each chart component.
 */
export const CELL_CHART_HEIGHT = 140; // px
export const CELL_CHART_HEIGHT_TALL = Math.round(CELL_CHART_HEIGHT * 2.5); // 350 px
export const CELL_CHART_MIN_WIDTH = 200; // px
/** Fixed width of the left info panel in every cell row (asset, tariff, accumulated).
 * Keeping this identical across all rows aligns the chart left edges (y-axes). */
export const CELL_LEFT_SECTION_WIDTH = 196; // px

/** Height for full-page diagnostic charts (Raw Diagnostics page) — deliberately taller
 * than CELL_CHART_HEIGHT since these are standalone views, not dashboard cells. */
export const DIAGNOSTIC_CHART_HEIGHT = 260; // px

/** Default time window: 1 h back, 1 h forward from now. */
export const DEFAULT_WINDOW = { hoursBack: 1.0, hoursForward: 1.0 };
/** Extended time window: 1 h back, 48 h forward (full plan horizon). */
export const EXTENDED_WINDOW = { hoursBack: 1.0, hoursForward: 48.0 };

/** X-axis tick spacing [minutes] for the default (2h-span) cell view — rounded to the
 * wall-clock via `roundedTimeTicks`, same mechanism as the History page. */
export const DEFAULT_TICK_INTERVAL_MINUTES = 10;
/** X-axis tick spacing [minutes] for the extended (49h-span) cell view — falls back to
 * hourly automatically via `roundedTimeTicks`'s density guard. */
export const EXTENDED_TICK_INTERVAL_MINUTES = 30;
