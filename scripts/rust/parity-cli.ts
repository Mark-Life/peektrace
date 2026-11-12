/** The `cli` stage: `statusline`, `mcp` and `upgrade`. */
import { runStatuslineStage } from "./parity-statusline";

export const runCliStage = async ({
  fixtures,
  real,
}: {
  readonly fixtures: boolean;
  readonly real: boolean;
}) => {
  runStatuslineStage({ fixtures, real });
  await Promise.resolve();
};
