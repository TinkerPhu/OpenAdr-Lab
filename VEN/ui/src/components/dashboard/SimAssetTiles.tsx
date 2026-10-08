import { Grid, Stack, Typography } from "@mui/material";
import type { AssetSnapshot, SimSnapshot } from "../../api/types";
import { ASSET_LABELS } from "../controller/types";
import { formatFixedOrDash } from "@lab/charts/unitFormat";

type Field = (a: AssetSnapshot) => string;

const power: Field = (a) => `Power: ${formatFixedOrDash(a.power_kw, 1)} kW`;
const soc: Field = (a) => `SOC: ${formatFixedOrDash((a.soc ?? 0) * 100, 1)}%`;

/**
 * What each asset kind shows beyond its label. A kind absent from this table still gets a
 * tile (label + power), so a new asset type is visible here without editing this file.
 */
const EXTRA_FIELDS: Record<string, Field[]> = {
  ev: [soc, power, (a) => `Plugged: ${(a.plugged ?? 0) !== 0 ? "Yes" : "No"}`],
  heater: [(a) => `Temp: ${formatFixedOrDash(a.temp_c, 1)}°C`, power],
  pv: [
    (a) => `Output: ${formatFixedOrDash(a.power_kw, 1)} kW`,
    (a) => `Irradiance: ${formatFixedOrDash((a.irradiance ?? 0) * 100, 0)}%`,
    (a) => `Generation limit: ${"generation_limit_kw" in a ? `${formatFixedOrDash(a.generation_limit_kw ?? 0, 1)} kW` : "none"}`,
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
