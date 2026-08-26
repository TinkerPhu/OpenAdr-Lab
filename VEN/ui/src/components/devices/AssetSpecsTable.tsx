import {
  Card,
  CardContent,
  CardHeader,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
} from "@mui/material";
import { deriveAssetSummaries } from "../controller/dataBuilders";
import { formatEnergyKwh, formatPowerValue } from "../charts/unitFormat";
import type { SimSnapshot } from "../../api/types";

interface AssetSpecsTableProps {
  sim: SimSnapshot | undefined;
}

/**
 * Static nameplate specs (capacity, max import/export) per asset — moved here
 * from the Controller tab's per-asset cell (see `AssetLeftSection`), where a
 * variable-length "Specs:" line desynced that cell's left-section height from
 * its chart's height across asset cells, breaking diagram alignment there.
 *
 * Reuses `deriveAssetSummaries` (the same derivation Controller uses) with
 * empty tariffs/requests/timelines — those inputs only affect the live
 * power/cost fields this table doesn't render, not the nameplate specs.
 */
export function AssetSpecsTable({ sim }: AssetSpecsTableProps) {
  if (!sim) return null;

  // `nowMs` only affects forecastEnergyKwh/activeRequest — neither rendered
  // here — so a fixed constant (not Date.now(), an impure call disallowed at
  // render time) keeps this derivation pure without changing which rows show.
  const rows = deriveAssetSummaries(sim, [], [], {}, 0).filter(
    (s) => s.capacityKwh !== null || s.maxImportKw !== null || s.maxExportKw !== null
  );

  if (rows.length === 0) return null;

  return (
    <Card variant="outlined" data-testid="asset-specs-table">
      <CardHeader title="Device Specs" />
      <CardContent>
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>Device</TableCell>
              <TableCell>Capacity</TableCell>
              <TableCell>Max in</TableCell>
              <TableCell>Max out</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {rows.map((s) => (
              <TableRow key={s.assetId} data-testid={`asset-specs-row-${s.assetId}`}>
                <TableCell>{s.label}</TableCell>
                <TableCell>{s.capacityKwh !== null ? formatEnergyKwh(s.capacityKwh) : "—"}</TableCell>
                <TableCell>{s.maxImportKw !== null ? formatPowerValue(s.maxImportKw) : "—"}</TableCell>
                <TableCell>{s.maxExportKw !== null ? formatPowerValue(s.maxExportKw) : "—"}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
}
