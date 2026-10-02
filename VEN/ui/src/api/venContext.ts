//! Which VEN this browser tab is pointed at.
//!
//! The context object and its hook, separate from `App.tsx` so that file
//! exports only components (`react-refresh/only-export-components`). The
//! provider stays in `App.tsx`, which is where the VEN selector that drives
//! it lives; every consumer only ever wanted the hook.

import { createContext, useContext } from "react";

import type { VenApi } from "./client";

export type VenContextType = {
  venUrl: string;
  venName: string;
  setVenUrl: (url: string) => void;
  api: VenApi;
};

export const VenContext = createContext<VenContextType | null>(null);

export function useVenContext(): VenContextType {
  const ctx = useContext(VenContext);
  if (!ctx) throw new Error("useVenContext must be used within VenProvider");
  return ctx;
}
