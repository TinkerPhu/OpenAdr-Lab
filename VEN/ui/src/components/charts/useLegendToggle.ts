import { useCallback, useState } from "react";

/**
 * Local (not persisted, not shared across chart instances) toggle state for interactive
 * chart legends — a set of hidden series keys. Every series starts visible (empty set).
 */
export function useLegendToggle() {
  const [hidden, setHidden] = useState<Set<string>>(() => new Set());

  const isHidden = useCallback((key: string) => hidden.has(key), [hidden]);

  const toggle = useCallback((key: string) => {
    setHidden((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next;
    });
  }, []);

  return { isHidden, toggle };
}
