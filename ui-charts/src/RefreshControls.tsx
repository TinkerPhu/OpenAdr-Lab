import { useState } from "react";
import { Button } from "@mui/material";
import { useQueryClient } from "@tanstack/react-query";

/**
 * The "Auto" toggle and "Refresh" button in the VEN and VTN toolbars. They were two verbatim
 * copies (state, handlers and markup) in each `App.tsx`; this is the one implementation.
 * Covered by `VEN/ui/src/__tests__/RefreshControls.test.tsx` (shared source is tested from the
 * VEN workspace).
 *
 * The toggle works on the query client's *defaults*: switching off sets
 * `refetchInterval: false` for every query, switching on clears it so each query's own interval
 * resumes. Both refetch once immediately.
 */
function useAutoRefresh() {
  const [autoRefresh, setAutoRefresh] = useState(true);
  const queryClient = useQueryClient();

  function refreshAll() {
    queryClient.invalidateQueries();
  }

  function toggleAutoRefresh() {
    setAutoRefresh((a) => !a);
    if (autoRefresh) {
      // Turning off: set all refetch intervals to false.
      queryClient.setDefaultOptions({
        queries: { refetchInterval: false },
      });
    } else {
      // Turning on: clear the defaults so per-query intervals resume.
      queryClient.setDefaultOptions({
        queries: { refetchInterval: undefined },
      });
    }
    queryClient.invalidateQueries();
  }

  return { autoRefresh, toggleAutoRefresh, refreshAll };
}

export function RefreshControls() {
  const { autoRefresh, toggleAutoRefresh, refreshAll } = useAutoRefresh();
  return (
    <>
      <Button
        color="inherit"
        onClick={toggleAutoRefresh}
        data-testid="auto-refresh-toggle"
        aria-label={`Auto refresh: ${autoRefresh ? "On" : "Off"}`}
        aria-pressed={autoRefresh}
      >
        Auto: {autoRefresh ? "On" : "Off"}
      </Button>
      <Button
        color="inherit"
        onClick={refreshAll}
        data-testid="refresh-all-btn"
        aria-label="Refresh all data"
      >
        Refresh
      </Button>
    </>
  );
}
