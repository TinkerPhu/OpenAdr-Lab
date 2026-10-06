import { Grid, Stack, Typography } from "@mui/material";
import type { AssetSnapshot, SimSnapshot } from "../../api/types";
import { ASSET_LABELS } from "../controller/types";

function fmt(v: number | null | undefined, decimals = 1): string {
  return v === null || v === undefined || Number.isNaN(v) ? "-" : v.toFixed(decimals);
}

type Field = (a: AssetSnapshot) => string;

const power: Field = (a) => `Power: ${fmt(a.power_kw)} kW`;
const soc: Field = (a) => `SOC: ${fmt((a.soc ?? 0) * 100)}%`;

/**
 * What each asset kind shows beyond its label. A kind absent from this table still gets a
 * tile (label + power), so a new asset type is visible here without editing this file.
 */
const EXTRA_FIELDS: Record<string, Field[]> = {
  ev: [soc, power, (a) => `Plugged: ${(a.plugged ?? 0) !== 0 ? "Yes" : "No"}`],
  heater: [(a) => `Temp: ${fmt(a.temp_c)}°C`, power],
  pv: [
    (a) => `Output: ${fmt(a.power_kw)} kW`,
    (a) => `Irradiance: ${fmt((a.irradiance ?? 0) * 100, 0)}%`,
    (a) => `Generation limit: ${"generation_limit_kw" in a ? `${fmt(a.generation_limit_kw ?? 0)} kW` : "none"}`,
  ],
  battery: [soc, power],
};

export function SimAssetTiles({ assets }: { assets: SimSnapshot["assets"] }) {
  return (
    <>
      {Object.entries(assets).map(([id, asset]) => (
        <Grid item xs={4} key={id}>
          <Stack spacing={0.5} data-testid={`sim-asset-${id}`}>
            <Typography variant="subtitle2">{ASSET_LABELS[id] ?? id.toUpperCase()}</Typography>
            {(EXTRA_FIELDS[id] ?? [power]).map((field, i) => (
              <Typography key={i}>{field(asset)}</Typography>
            ))}
          </Stack>
        </Grid>
      ))}
    </>
  );
}
