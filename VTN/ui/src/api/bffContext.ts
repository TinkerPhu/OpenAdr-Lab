import { createContext, useContext } from "react";
import type { BffApi } from "./client";

/** The BFF client every hook reads, provided once by `App`. Lives outside `App.tsx`
 * so that file exports only components (react-refresh/only-export-components). */
export type BffContextType = {
  api: BffApi;
};

export const BffContext = createContext<BffContextType | null>(null);

export function useBffContext(): BffContextType {
  const ctx = useContext(BffContext);
  if (!ctx) throw new Error("useBffContext must be used within BffProvider");
  return ctx;
}
