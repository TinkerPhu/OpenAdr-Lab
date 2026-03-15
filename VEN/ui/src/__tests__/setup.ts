import "@testing-library/jest-dom/vitest";

// recharts ResponsiveContainer uses ResizeObserver which jsdom doesn't provide
(globalThis as typeof globalThis & { ResizeObserver: unknown }).ResizeObserver =
  class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
