import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SensorForm } from "../components/SensorForm";

const mutateMock = vi.fn();
const usePostSensorMock = vi.fn(() => ({
  mutate: mutateMock,
  isPending: false,
  isSuccess: false,
  isError: false,
  error: null,
}));

vi.mock("../api/hooks", () => ({
  usePostSensor: () => usePostSensorMock(),
}));

vi.mock("../App", () => ({
  useVenContext: () => ({
    venUrl: "http://localhost:8081",
    setVenUrl: vi.fn(),
    api: {
      baseUrl: "http://localhost:8081",
      postSensors: vi.fn().mockResolvedValue({}),
    },
  }),
}));

function renderForm() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <SensorForm />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("SensorForm", () => {
  beforeEach(() => {
    mutateMock.mockClear();
    usePostSensorMock.mockReturnValue({
      mutate: mutateMock,
      isPending: false,
      isSuccess: false,
      isError: false,
      error: null,
    });
  });

  it("renders all form inputs", () => {
    renderForm();
    expect(screen.getByTestId("sensor-form-temp")).toBeVisible();
    expect(screen.getByTestId("sensor-form-power")).toBeVisible();
    expect(screen.getByTestId("sensor-form-voltage")).toBeVisible();
    expect(screen.getByTestId("sensor-form-raw")).toBeVisible();
    expect(screen.getByTestId("sensor-form-submit")).toBeVisible();
  });

  it("submits form with entered values", async () => {
    renderForm();
    await userEvent.type(screen.getByTestId("sensor-form-temp"), "25.5");
    await userEvent.type(screen.getByTestId("sensor-form-power"), "200");
    await userEvent.type(screen.getByTestId("sensor-form-voltage"), "240");
    await userEvent.click(screen.getByTestId("sensor-form-submit"));
    expect(mutateMock).toHaveBeenCalledWith({
      temperature_c: 25.5,
      power_w: 200,
      voltage_v: 240,
    });
  });

  it("shows success message after submission", () => {
    usePostSensorMock.mockReturnValue({
      mutate: mutateMock,
      isPending: false,
      isSuccess: true,
      isError: false,
      error: null,
    });
    renderForm();
    expect(screen.getByTestId("sensor-form-success")).toBeVisible();
    expect(screen.getByTestId("sensor-form-success")).toHaveTextContent("successfully");
  });

  it("disables submit button when pending", () => {
    usePostSensorMock.mockReturnValue({
      mutate: mutateMock,
      isPending: true,
      isSuccess: false,
      isError: false,
      error: null,
    });
    renderForm();
    expect(screen.getByTestId("sensor-form-submit")).toBeDisabled();
  });
});
