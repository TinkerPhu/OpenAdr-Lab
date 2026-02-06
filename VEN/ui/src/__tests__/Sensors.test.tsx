import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { BrowserRouter } from "react-router-dom";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SensorsPage } from "../pages/Sensors";

const mockSensor = {
  id: "s1",
  ts: "2024-01-01T12:00:00Z",
  temperature_c: 22.5,
  power_w: 150,
  voltage_v: 230,
  raw: { device: "test-device" },
};

const useSensorMock = vi.fn(() => ({
  data: mockSensor as typeof mockSensor | undefined,
  dataUpdatedAt: Date.now(),
}));

const usePostSensorMock = vi.fn(() => ({
  mutate: vi.fn(),
  isPending: false,
  isSuccess: false,
  isError: false,
  error: null,
}));

vi.mock("../api/hooks", () => ({
  useSensor: () => useSensorMock(),
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

function renderSensors() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <SensorsPage />
      </BrowserRouter>
    </QueryClientProvider>,
  );
}

describe("SensorsPage", () => {
  beforeEach(() => {
    useSensorMock.mockReturnValue({
      data: mockSensor,
      dataUpdatedAt: Date.now(),
    });
    usePostSensorMock.mockReturnValue({
      mutate: vi.fn(),
      isPending: false,
      isSuccess: false,
      isError: false,
      error: null,
    });
  });

  it("renders heading and last updated", () => {
    renderSensors();
    expect(screen.getByTestId("sensors-heading")).toBeVisible();
    expect(screen.getByTestId("sensors-heading")).toHaveTextContent("Sensors");
    expect(screen.getByTestId("sensors-last-updated")).toBeVisible();
  });

  it("displays sensor data", () => {
    renderSensors();
    expect(screen.getByTestId("sensor-timestamp")).toHaveTextContent("2024-01-01T12:00:00Z");
    expect(screen.getByTestId("sensor-power")).toHaveTextContent("150");
    expect(screen.getByTestId("sensor-temp")).toHaveTextContent("22.5");
    expect(screen.getByTestId("sensor-voltage")).toHaveTextContent("230");
  });

  it("opens raw JSON dialog", async () => {
    renderSensors();
    await userEvent.click(screen.getByTestId("sensor-raw-link"));
    expect(screen.getByTestId("json-dialog")).toBeVisible();
    expect(screen.getByTestId("json-dialog-content")).toHaveTextContent("test-device");
  });

  it("shows dash when no sensor data", () => {
    useSensorMock.mockReturnValue({ data: undefined, dataUpdatedAt: 0 });
    renderSensors();
    expect(screen.getByTestId("sensor-timestamp")).toHaveTextContent("—");
    expect(screen.getByTestId("sensor-power")).toHaveTextContent("—");
    expect(screen.getByTestId("sensor-temp")).toHaveTextContent("—");
    expect(screen.getByTestId("sensor-voltage")).toHaveTextContent("—");
  });

  it("includes the sensor form", () => {
    renderSensors();
    expect(screen.getByTestId("sensor-form-submit")).toBeVisible();
  });
});
