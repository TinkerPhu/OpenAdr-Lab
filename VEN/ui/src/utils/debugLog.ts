// Dev-only console logging. `import.meta.env.DEV` is false in production builds,
// so these calls compile to no-ops in shipped bundles.
export function debugLog(...args: unknown[]): void {
  if (import.meta.env.DEV) {
    console.log(...args);
  }
}
