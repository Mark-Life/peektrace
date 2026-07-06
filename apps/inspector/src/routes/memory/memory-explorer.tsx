/** The all-projects memory explorer (Phase 7.1).
 *
 * Default scope = every Claude vault on the machine: a project overview row on
 * top, then each project's full forensic + CRUD surface in an accordion. Search
 * + type-filter span all projects; a scope select drills into one project. An
 * agent selector drives the capability gate (non-Claude → read-only banner, no
 * write affordances). Writes refresh `allVaultsAtom` so gauges re-validate.
 */
import { useAtomRefresh, useAtomValue } from "@effect-atom/atom-react";
import type { AgentId } from "@workspace/core/services/agent-id";
import type {
  AllVaults,
  MemoryEntry,
  MemoryVault,
} from "@workspace/core/services/memory/types";
import { MEMORY_TYPES } from "@workspace/core/services/memory/types";
import type { Capability } from "@workspace/rpc/contract";
import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@workspace/ui/components/accordion";
import { Badge } from "@workspace/ui/components/badge";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@workspace/ui/components/card";
import { Input } from "@workspace/ui/components/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select";
import { useMemo, useState } from "react";
import { allVaultsAtom, capabilitiesAtom } from "../../lib/atoms";
import { memoryCrudVerdict } from "../../lib/memory-atoms";
import { ResultView } from "../../lib/result-view";
import { CapabilityBanner } from "./capability-banner";
import { VaultSection } from "./vault-section";

/** Agent options for the capability-gate selector. */
const AGENTS: readonly { id: AgentId; label: string }[] = [
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
  { id: "pi", label: "Pi" },
  { id: "opencode", label: "OpenCode" },
];

/** Does an entry match the free-text query? */
const matchesQuery = (entry: MemoryEntry, q: string) => {
  if (q.length === 0) {
    return true;
  }
  const hay =
    `${entry.slug} ${entry.name ?? ""} ${entry.description ?? ""} ${entry.body}`.toLowerCase();
  return hay.includes(q.toLowerCase());
};

/** Apply search + type filter to a vault, returning its matching entries. */
const filterEntries = ({
  vault,
  query,
  type,
}: {
  readonly vault: MemoryVault;
  readonly query: string;
  readonly type: string;
}) =>
  vault.entries.filter(
    (e) =>
      matchesQuery(e, query) &&
      (type === "all" || (e.type ?? "untyped") === type)
  );

/** Project overview cards across all discovered vaults. */
const Overview = ({ all }: { readonly all: typeof AllVaults.Type }) => (
  <div
    className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4"
    data-testid="memory-overview"
  >
    {all.projects.map((p) => (
      <Card key={p.slug}>
        <CardHeader className="pb-2">
          <CardTitle className="truncate text-sm">{p.project}</CardTitle>
          <CardDescription className="truncate font-mono text-xs">
            {p.slug}
          </CardDescription>
        </CardHeader>
        <CardContent className="text-muted-foreground text-xs">
          {p.fileCount} file{p.fileCount === 1 ? "" : "s"}
          {p.hasIndex ? " · indexed" : " · no MEMORY.md"}
        </CardContent>
      </Card>
    ))}
  </div>
);

/** Inner explorer once both queries resolved. */
const Explorer = ({
  all,
  caps,
}: {
  readonly all: typeof AllVaults.Type;
  readonly caps: readonly Capability[];
}) => {
  const refresh = useAtomRefresh(allVaultsAtom);
  const [agent, setAgent] = useState<AgentId>("claude");
  const [query, setQuery] = useState("");
  const [type, setType] = useState("all");
  const [scope, setScope] = useState("all");

  const verdict = memoryCrudVerdict({ caps, agent });
  const canWrite = verdict.canWrite;

  const vaults = useMemo(
    () =>
      all.vaults
        .filter((v) => scope === "all" || v.slug === scope)
        .map((vault) => ({
          vault,
          entries: filterEntries({ vault, query, type }),
        }))
        .filter(({ entries }) =>
          query.length === 0 && type === "all" ? true : entries.length > 0
        ),
    [all.vaults, scope, query, type]
  );

  return (
    <div className="flex flex-col gap-5" data-testid="memory-explorer">
      <div className="flex flex-wrap items-center gap-3">
        <Input
          className="max-w-xs"
          data-testid="memory-search"
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search all memories…"
          value={query}
        />
        <Select onValueChange={setType} value={type}>
          <SelectTrigger className="w-36" data-testid="memory-type-filter">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All types</SelectItem>
            {MEMORY_TYPES.map((t) => (
              <SelectItem key={t} value={t}>
                {t}
              </SelectItem>
            ))}
            <SelectItem value="untyped">untyped</SelectItem>
          </SelectContent>
        </Select>
        <Select onValueChange={setScope} value={scope}>
          <SelectTrigger className="w-52" data-testid="memory-scope">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All projects</SelectItem>
            {all.vaults.map((v) => (
              <SelectItem key={v.slug} value={v.slug}>
                {v.project}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <div className="ml-auto flex items-center gap-2">
          <span className="text-muted-foreground text-xs">Agent</span>
          <Select onValueChange={(v) => setAgent(v as AgentId)} value={agent}>
            <SelectTrigger className="w-32" data-testid="agent-select">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {AGENTS.map((a) => (
                <SelectItem key={a.id} value={a.id}>
                  {a.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      </div>

      {canWrite ? null : (
        <CapabilityBanner
          agent={agent}
          level={verdict.level}
          note={verdict.note}
        />
      )}

      <Overview all={all} />

      {vaults.length === 0 ? (
        <p className="text-muted-foreground text-sm" data-testid="no-matches">
          No memories match the current filters.
        </p>
      ) : (
        <Accordion
          defaultValue={vaults.map(({ vault }) => vault.slug)}
          type="multiple"
        >
          {vaults.map(({ vault, entries }) => (
            <AccordionItem key={vault.slug} value={vault.slug}>
              <AccordionTrigger className="px-4">
                <span className="flex items-center gap-2">
                  <span className="font-medium">{vault.project}</span>
                  <Badge variant="secondary">{entries.length}</Badge>
                  {vault.budget.overBudget ? (
                    <Badge variant="destructive">over budget</Badge>
                  ) : null}
                </span>
              </AccordionTrigger>
              <AccordionContent className="px-4 pb-4">
                <VaultSection
                  canWrite={canWrite}
                  entries={entries}
                  onRefresh={refresh}
                  vault={vault}
                />
              </AccordionContent>
            </AccordionItem>
          ))}
        </Accordion>
      )}
    </div>
  );
};

/** Resolve both queries, then render the explorer. */
export const MemoryExplorer = () => {
  const allResult = useAtomValue(allVaultsAtom);
  const capsResult = useAtomValue(capabilitiesAtom);
  return (
    <ResultView result={allResult}>
      {(all) =>
        all.projects.length === 0 ? (
          <Card data-testid="memory-empty">
            <CardHeader>
              <CardTitle>No memories found</CardTitle>
              <CardDescription>
                No project under the Claude projects root has a non-empty
                memory/ directory yet.
              </CardDescription>
            </CardHeader>
          </Card>
        ) : (
          <ResultView result={capsResult}>
            {(caps) => <Explorer all={all} caps={caps} />}
          </ResultView>
        )
      }
    </ResultView>
  );
};
