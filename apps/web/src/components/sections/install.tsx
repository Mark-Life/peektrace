import { Alert, AlertDescription } from "@workspace/ui/components/alert";
import { Card } from "@workspace/ui/components/card";
import { Info } from "lucide-react";
import { CommandBlock } from "@/components/command-block";
import { InstallTabs } from "@/components/sections/install-tabs";

interface RefEntry {
  desc: string;
  name: string;
}

interface QuickstartStep {
  commands: string[];
  title: string;
}

const QUICKSTART: QuickstartStep[] = [
  {
    title: "Boot the inspector — opens your browser, loopback only",
    commands: ["peektrace serve"],
  },
  {
    title: "Or score a session without leaving the terminal",
    commands: ["peektrace sessions ls", "peektrace sessions analyze <id>"],
  },
  {
    title: "Audit your memory index",
    commands: ["peektrace memory ls"],
  },
  {
    title: "Pipe any of it into your own tools",
    commands: ["peektrace --json sessions analyze <id> | jq .verdict"],
  },
];

const GLOBAL_FLAGS: RefEntry[] = [
  { name: "--json", desc: "raw RPC JSON instead of tables" },
  {
    name: "--pretty",
    desc: "aligned tables (default output is compact, tab-separated)",
  },
  { name: "--read-only", desc: "refuse every mutating command up front" },
  {
    name: "--remote <url>",
    desc: "run against a running serve instead of in-process",
  },
  { name: "--otel", desc: "stream Effect spans to stderr" },
  { name: "--no-telemetry", desc: "disable local telemetry for this run" },
  { name: "-v, --version", desc: "print version" },
];

const COMMANDS: RefEntry[] = [
  {
    name: "serve",
    desc: "boot the loopback inspector UI + RPC server (--port, --host, --open/--no-open, --read-only)",
  },
  { name: "sessions ls", desc: "list session headers (--agent, --project)" },
  { name: "sessions analyze <id>", desc: "context forensics for one session" },
  {
    name: "memory ls [project]",
    desc: "projects with memory, or one vault's entries",
  },
  {
    name: "memory show <project> <name>",
    desc: "frontmatter + full body of one entry",
  },
  {
    name: "memory rm <project> <name>",
    desc: "delete an entry (blocked under --read-only)",
  },
  {
    name: "doctor",
    desc: "write a redacted support bundle (--last, --interesting-only, --out)",
  },
];

/**
 * Renders one flag/command reference row as an aligned two-column grid that
 * collapses to a stacked pair on mobile.
 */
const ReferenceRow = ({ name, desc }: RefEntry) => (
  <div className="grid grid-cols-1 gap-x-4 gap-y-0.5 py-1 sm:grid-cols-[minmax(0,15rem)_1fr]">
    <code className="text-primary">{name}</code>
    <span className="text-muted-foreground">{desc}</span>
  </div>
);

/**
 * Install & first run: the native one-liner installer (the only distribution
 * channel — Peektrace is not on npm and ships no desktop app), a numbered
 * quickstart, and a man-page-style command reference.
 */
export const Install = () => (
  <section className="border-border/60 border-t bg-muted/30 py-24" id="install">
    <div className="mx-auto max-w-6xl px-6">
      <p className="font-mono text-primary text-xs uppercase tracking-[0.2em]">
        Get started
      </p>
      <h2 className="mt-4 font-heading text-3xl tracking-tight sm:text-4xl">
        One binary. Thirty seconds to your first verdict.
      </h2>

      <div className="mt-8 max-w-4xl space-y-4">
        <InstallTabs />
        <p className="text-muted-foreground text-sm/relaxed">
          Pulls the prebuilt binary from GitHub Releases, checks its SHA256
          against SHA256SUMS (and refuses to install on a mismatch), then drops
          it on your PATH:{" "}
          <code className="font-mono text-foreground">~/.local/bin</code> on
          macOS and Linux,{" "}
          <code className="font-mono text-foreground">
            %LOCALAPPDATA%\peektrace\bin
          </code>{" "}
          on Windows. macOS (arm64, x64), Linux (x64), Windows (x64).
        </p>
      </div>

      <div className="mt-14">
        <h3 className="font-heading text-xl">First run</h3>
        <ol className="mt-6 space-y-4">
          {QUICKSTART.map((step, index) => (
            <li key={step.title}>
              <Card className="gap-4 p-5">
                <div className="flex items-start gap-3">
                  <span
                    aria-hidden="true"
                    className="grid size-6 shrink-0 place-items-center rounded-full bg-primary font-mono text-primary-foreground text-xs"
                  >
                    {index + 1}
                  </span>
                  <p className="font-medium leading-6">{step.title}</p>
                </div>
                <div className="space-y-2">
                  {step.commands.map((command) => (
                    <CommandBlock command={command} key={command} />
                  ))}
                </div>
              </Card>
            </li>
          ))}
        </ol>
      </div>

      <div className="mt-14">
        <h3 className="font-heading text-xl">Command reference</h3>
        <Card className="mt-6 gap-6 p-6 font-mono text-sm">
          <div>
            <p className="mb-3 text-muted-foreground text-xs uppercase tracking-[0.15em]">
              Global flags (must precede the subcommand)
            </p>
            <div className="divide-y divide-border/50">
              {GLOBAL_FLAGS.map((entry) => (
                <ReferenceRow
                  desc={entry.desc}
                  key={entry.name}
                  name={entry.name}
                />
              ))}
            </div>
          </div>
          <div>
            <p className="mb-3 text-muted-foreground text-xs uppercase tracking-[0.15em]">
              Commands
            </p>
            <div className="divide-y divide-border/50">
              {COMMANDS.map((entry) => (
                <ReferenceRow
                  desc={entry.desc}
                  key={entry.name}
                  name={entry.name}
                />
              ))}
            </div>
          </div>
        </Card>
      </div>

      <Alert className="mt-8" variant="default">
        <Info aria-hidden="true" />
        <AlertDescription>
          Not on npm, no desktop download — the native installer is the only
          channel. It also works read-only over an SSH tunnel to a headless VPS:{" "}
          <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-foreground">
            peektrace --remote http://127.0.0.1:4321 sessions ls
          </code>
          .
        </AlertDescription>
      </Alert>
    </div>
  </section>
);
