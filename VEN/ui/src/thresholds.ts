/**
 * Named thresholds for floating-point comparisons.
 *
 * Mirror of VEN/src/controller/thresholds.rs — keep values in sync.
 */

/**
 * Power below this is treated as "effectively zero" for display and cost
 * attribution purposes.  1 W — below any real meter resolution.
 * Prevents recharts from auto-scaling the Y-axis to a micro-range on
 * near-zero residuals (chart needle bug).
 */
export const NEAR_ZERO_KW = 0.001;
