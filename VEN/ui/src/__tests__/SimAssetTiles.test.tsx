import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import type { SimSnapshot } from "../api/types";
import { SimAssetTiles } from "../components/dashboard/SimAssetTiles";

const assets = (a: SimSnapshot["assets"]) => a;

describe("SimAssetTiles", () => {
  it("shows a tile for every asset, including kinds with no special fields", () => {
    render(
      <SimAssetTiles
        assets={assets({
          battery: { power_kw: 1.5, soc: 0.5 },
          base_load: { power_kw: 0.4 },
          wm: { power_kw: 0 },
        })}
      />,
    );
    expect(screen.getByText("Battery")).toBeTruthy();
    expect(screen.getByText("Base Load")).toBeTruthy();
    expect(screen.getByText("Washing Machine")).toBeTruthy();
  });

  it("shows the declared extra fields of a known kind", () => {
    render(<SimAssetTiles assets={assets({ battery: { power_kw: 1.5, soc: 0.5 } })} />);
    expect(screen.getByText("SOC: 50.0%")).toBeTruthy();
    expect(screen.getByText("Power: 1.5 kW")).toBeTruthy();
  });

  it("falls back to power for an asset kind it has never heard of", () => {
    render(<SimAssetTiles assets={assets({ sauna: { power_kw: 2 } })} />);
    expect(screen.getByText("SAUNA")).toBeTruthy();
    expect(screen.getByText("Power: 2.0 kW")).toBeTruthy();
  });

  it("keeps the EV, heater and PV fields the card showed before", () => {
    render(
      <SimAssetTiles
        assets={assets({
          ev: { power_kw: 7, soc: 0.25, plugged: 1 },
          heater: { power_kw: 2, temp_c: 55 },
          pv: { power_kw: 3, irradiance: 0.8 },
        })}
      />,
    );
    expect(screen.getByText("Plugged: Yes")).toBeTruthy();
    expect(screen.getByText("Temp: 55.0°C")).toBeTruthy();
    expect(screen.getByText("Irradiance: 80%")).toBeTruthy();
    expect(screen.getByText("Generation limit: none")).toBeTruthy();
  });
});
