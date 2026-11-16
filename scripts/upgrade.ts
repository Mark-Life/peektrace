import { $ } from "bun";

const SEPARATOR_WIDTH = 50;

const steps = [
  {
    command: () =>
      $`bun add -D --exact oxlint@latest @oxlint/plugins@latest oxlint-tsgolint@latest oxfmt@latest typescript@latest tsrs@latest ultracite@latest`,
    critical: true,
    name: "Bump root dev tooling",
  },
  {
    command: () =>
      $`bun add lucide-react@latest react-day-picker@latest && bun add -D typescript@latest`.cwd(
        "packages/ui"
      ),
    critical: true,
    name: "Bump packages/ui majors",
  },
  {
    command: () =>
      $`bunx shadcn@latest add --all --overwrite`.cwd("packages/ui"),
    critical: true,
    name: "shadcn/ui Components",
  },
  {
    command: () => $`bun update`,
    critical: true,
    name: "Dependency Update",
  },
  {
    command: () => $`bun run fix`,
    critical: false,
    name: "Ultracite Fix",
  },
  {
    command: () => $`bun run typecheck`,
    critical: false,
    name: "Type Check",
  },
] as const;

let failed = false;

for (const step of steps) {
  console.log(`\n${"=".repeat(SEPARATOR_WIDTH)}`);
  console.log(`>> ${step.name}`);
  console.log("=".repeat(SEPARATOR_WIDTH));

  // Steps run one after another: each may abort the run.
  // oxlint-disable-next-line no-await-in-loop -- sequential by design
  const result = await step.command().nothrow();

  if (result.exitCode === 0) {
    console.log(`\n✓ ${step.name} completed`);
  } else {
    console.error(`\n!! ${step.name} failed (exit code ${result.exitCode})`);

    if (step.critical) {
      console.error("Critical step failed, aborting.");
      process.exit(1);
    }

    failed = true;
    console.warn("Non-critical failure, continuing...");
  }
}

if (failed) {
  console.warn("\nUpgrade completed with warnings.");
  process.exit(1);
}

console.log("\nUpgrade completed successfully.");
