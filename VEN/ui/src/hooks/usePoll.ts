import { useEffect, useRef } from "react";

export function usePoll(fn: () => void | Promise<void>, ms: number, enabled: boolean) {
  const fnRef = useRef(fn);
  fnRef.current = fn;

  useEffect(() => {
    if (!enabled) return;
    fnRef.current();
    const id = setInterval(() => fnRef.current(), ms);
    return () => clearInterval(id);
  }, [ms, enabled]);
}
