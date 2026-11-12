/** The `cli` stage: `statusline`, `mcp`, `upgrade`, the owned help pages
 * and telemetry. */
import { runHelpStage } from "./parity-help";
import { runMcpStage } from "./parity-mcp";
import { runStatuslineStage } from "./parity-statusline";
import { runTelemetryStage } from "./parity-telemetry";
import { runUpgradeStage } from "./parity-upgrade";

export const runCliStage = async ({
  fixtures,
  real,
}: {
  readonly fixtures: boolean;
  readonly real: boolean;
}) => {
  runStatuslineStage({ fixtures, real });
  await runMcpStage({ fixtures, real });
  if (fixtures) {
    await runUpgradeStage();
    runHelpStage();
    runTelemetryStage();
  }
};
