import { $ } from "bun";

const SEPARATOR_WIDTH = 50;

const steps = [
  {
    name: "Bump root dev tooling",
    command: () =>
      $`bun add -D --exact @biomejs/biome@latest typescript@latest ultracite@latest`,
    critical: true,
  },
  {
    name: "Bump packages/ui majors",
    command: () =>
      $`bun add lucide-react@latest react-day-picker@latest && bun add -D typescript@latest`.cwd(
        "packages/ui"
      ),
    critical: true,
  },
  {
    name: "shadcn/ui Components",
    command: () =>
      $`bunx shadcn@latest add --all --overwrite`.cwd("packages/ui"),
    critical: true,
  },
  {
    name: "Dependency Update",
    command: () => $`bun update`,
    critical: true,
  },
  {
    name: "Ultracite Fix",
    command: () => $`bun run fix`,
    critical: false,
  },
  {
    name: "Type Check",
    command: () => $`bun run typecheck`,
    critical: false,
  },
] as const;

let failed = false;

for (const step of steps) {
  console.log(`\n${"=".repeat(SEPARATOR_WIDTH)}`);
  console.log(`>> ${step.name}`);
  console.log("=".repeat(SEPARATOR_WIDTH));

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
