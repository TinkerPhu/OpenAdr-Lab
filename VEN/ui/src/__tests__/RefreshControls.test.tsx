import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { describe, it, expect, vi } from "vitest";
import { RefreshControls } from "@lab/charts/RefreshControls";

// The auto-refresh toggle and "Refresh" button both UIs' toolbars carry. They were two verbatim
// copies in App.tsx; these tests pin the exact sequence they performed.
function setup() {
  const queryClient = new QueryClient();
  const setDefaultOptions = vi.spyOn(queryClient, "setDefaultOptions");
  const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
  render(
    <QueryClientProvider client={queryClient}>
      <RefreshControls />
    </QueryClientProvider>,
  );
  return { setDefaultOptions, invalidateQueries };
}

describe("RefreshControls", () => {
  it("starts with auto refresh on", () => {
    setup();
    const toggle = screen.getByTestId("auto-refresh-toggle");
    expect(toggle).toHaveTextContent("Auto: On");
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    expect(toggle).toHaveAttribute("aria-label", "Auto refresh: On");
  });

  it("turning it off stops the default refetch interval and refetches once", async () => {
    const { setDefaultOptions, invalidateQueries } = setup();
    await userEvent.click(screen.getByTestId("auto-refresh-toggle"));

    expect(setDefaultOptions).toHaveBeenCalledTimes(1);
    expect(setDefaultOptions).toHaveBeenLastCalledWith({ queries: { refetchInterval: false } });
    expect(invalidateQueries).toHaveBeenCalledTimes(1);
    const toggle = screen.getByTestId("auto-refresh-toggle");
    expect(toggle).toHaveTextContent("Auto: Off");
    expect(toggle).toHaveAttribute("aria-pressed", "false");
  });

  it("turning it back on clears the default so each query's own interval resumes", async () => {
    const { setDefaultOptions, invalidateQueries } = setup();
    await userEvent.click(screen.getByTestId("auto-refresh-toggle"));
    await userEvent.click(screen.getByTestId("auto-refresh-toggle"));

    expect(setDefaultOptions).toHaveBeenCalledTimes(2);
    expect(setDefaultOptions).toHaveBeenLastCalledWith({ queries: { refetchInterval: undefined } });
    expect(invalidateQueries).toHaveBeenCalledTimes(2);
    expect(screen.getByTestId("auto-refresh-toggle")).toHaveTextContent("Auto: On");
  });

  it("Refresh invalidates every query and leaves the auto-refresh setting alone", async () => {
    const { setDefaultOptions, invalidateQueries } = setup();
    const button = screen.getByTestId("refresh-all-btn");
    expect(button).toHaveTextContent("Refresh");
    expect(button).toHaveAttribute("aria-label", "Refresh all data");

    await userEvent.click(button);

    expect(invalidateQueries).toHaveBeenCalledTimes(1);
    expect(invalidateQueries).toHaveBeenCalledWith();
    expect(setDefaultOptions).not.toHaveBeenCalled();
  });
});
