import "@testing-library/jest-dom/vitest";

// recharts ResponsiveContainer uses ResizeObserver which jsdom doesn't provide
global.ResizeObserver = class ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
};
