/** The all-projects memory explorer.
 *
 * Default scope = every Claude vault on the machine: a project overview row on
 * top, then each project's full forensic + CRUD surface in an accordion. Search
 * + type-filter span all projects; a scope select drills into one project.
 * "New memory in…" creates a memory in any Claude project, including the first
 * one when no vault exists yet. A read-only server or the `memory.crud` matrix
 * cell gates every write affordance. Writes refresh `allVaultsAtom` so gauges re-validate.
 */
import { useAtomRefresh, useAtomValue } from "@effect-atom/atom-react";
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
import { useEffect, useMemo, useRef, useState } from "react";
import {
  allVaultsAtom,
  capabilitiesAtom,
  serverInfoAtom,
} from "../../lib/atoms";
import {
  memoryCrudVerdict,
  memorySupportSummary,
} from "../../lib/memory-support";
import { ResultView } from "../../lib/result-view";
import { CapabilityBanner } from "./capability-banner";
import { CreateDialog } from "./create-dialog";
import { CreateAnywhereButton } from "./project-picker";
import { VaultSection } from "./vault-section";

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

/** Shown when no project has a memory folder with files. */
const EmptyState = ({
  canWrite,
  onCreate,
}: {
  readonly canWrite: boolean;
  readonly onCreate: () => void;
}) => (
  <Card data-testid="memory-empty">
    <CardHeader>
      <CardTitle>No memories yet</CardTitle>
      <CardDescription>
        No project has a memory folder with files yet.{" "}
        {canWrite
          ? "Create the first memory in a Claude project, or let Claude save one."
          : "Memories appear here once Claude saves one."}
      </CardDescription>
    </CardHeader>
    {canWrite ? (
      <CardContent>
        <CreateAnywhereButton onClick={onCreate} />
      </CardContent>
    ) : null}
  </Card>
);

/** Search, type and scope filters over the listed vaults. */
const Filters = ({
  all,
  query,
  type,
  scope,
  onQuery,
  onType,
  onScope,
}: {
  readonly all: typeof AllVaults.Type;
  readonly query: string;
  readonly type: string;
  readonly scope: string;
  readonly onQuery: (q: string) => void;
  readonly onType: (t: string) => void;
  readonly onScope: (s: string) => void;
}) => (
  <>
    <Input
      className="max-w-xs"
      data-testid="memory-search"
      onChange={(e) => onQuery(e.target.value)}
      placeholder="Search all memories…"
      value={query}
    />
    <Select onValueChange={onType} value={type}>
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
    <Select onValueChange={onScope} value={scope}>
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
  </>
);

/** Inner explorer once both queries resolved. */
export const Explorer = ({
  all,
  caps,
  readOnly,
}: {
  readonly all: typeof AllVaults.Type;
  readonly caps: readonly Capability[];
  readonly readOnly: boolean;
}) => {
  const refresh = useAtomRefresh(allVaultsAtom);
  const [query, setQuery] = useState("");
  const [type, setType] = useState("all");
  const [scope, setScope] = useState("all");
  const [creating, setCreating] = useState(false);
  // Vault whose trigger takes focus once it renders after a create.
  const [focusSlug, setFocusSlug] = useState<string | undefined>();
  const rootRef = useRef<HTMLDivElement>(null);
  // Collapsed vaults; every other vault (including a newly created one) is open.
  const [closed, setClosed] = useState<ReadonlySet<string>>(new Set());

  const verdict = memoryCrudVerdict({ caps, readOnly });
  const canWrite = verdict.blockedBy === null;
  const isEmpty = all.projects.length === 0;

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
  const visible = vaults.map(({ vault }) => vault.slug);

  const onOpenChange = (open: readonly string[]) =>
    setClosed(
      (prev) =>
        new Set([
          ...[...prev].filter((slug) => !visible.includes(slug)),
          ...visible.filter((slug) => !open.includes(slug)),
        ])
    );

  const onCreated = (slug: string) => {
    refresh();
    setQuery("");
    setType("all");
    setClosed((prev) => new Set([...prev].filter((s) => s !== slug)));
    if (scope !== "all" && scope !== slug) {
      setScope(slug);
    }
    setFocusSlug(slug);
  };

  // The empty state (and the button that opened the dialog) unmounts once the
  // first vault arrives, so focus moves to the new vault on purpose.
  useEffect(() => {
    if (
      focusSlug === undefined ||
      !vaults.some(({ vault }) => vault.slug === focusSlug)
    ) {
      return;
    }
    const trigger = rootRef.current?.querySelector<HTMLElement>(
      `[data-vault="${CSS.escape(focusSlug)}"]`
    );
    if (trigger) {
      trigger.focus();
      setFocusSlug(undefined);
    }
  }, [focusSlug, vaults]);

  return (
    <div
      className="flex flex-col gap-5"
      data-testid="memory-explorer"
      ref={rootRef}
    >
      {isEmpty ? null : (
        <div className="flex flex-wrap items-center gap-3">
          <Filters
            all={all}
            onQuery={setQuery}
            onScope={setScope}
            onType={setType}
            query={query}
            scope={scope}
            type={type}
          />
          {canWrite ? (
            <div className="ml-auto">
              <CreateAnywhereButton onClick={() => setCreating(true)} />
            </div>
          ) : null}
        </div>
      )}

      {verdict.blockedBy === null ? (
        <p
          className="text-muted-foreground text-xs"
          data-testid="memory-support"
        >
          {memorySupportSummary(caps)}
        </p>
      ) : (
        <CapabilityBanner
          blockedBy={verdict.blockedBy}
          detail={verdict.note ?? memorySupportSummary(caps)}
          level={verdict.level}
        />
      )}

      {isEmpty ? (
        <EmptyState canWrite={canWrite} onCreate={() => setCreating(true)} />
      ) : (
        <>
          <Overview all={all} />
          {vaults.length === 0 ? (
            <p
              className="text-muted-foreground text-sm"
              data-testid="no-matches"
            >
              No memories match the current filters.
            </p>
          ) : (
            <Accordion
              onValueChange={onOpenChange}
              type="multiple"
              value={visible.filter((slug) => !closed.has(slug))}
            >
              {vaults.map(({ vault, entries }) => (
                <AccordionItem key={vault.slug} value={vault.slug}>
                  <AccordionTrigger className="px-4" data-vault={vault.slug}>
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
        </>
      )}

      {canWrite ? (
        <CreateDialog
          onDone={onCreated}
          onOpenChange={setCreating}
          open={creating}
          target={{ kind: "pick", labels: all.labels }}
        />
      ) : null}
    </div>
  );
};

/** Resolve every query, then render the explorer (or its empty state). The
 * server mode is awaited so edit buttons never show and then vanish. */
export const MemoryExplorer = () => {
  const allResult = useAtomValue(allVaultsAtom);
  const capsResult = useAtomValue(capabilitiesAtom);
  const infoResult = useAtomValue(serverInfoAtom);
  const retryAll = useAtomRefresh(allVaultsAtom);
  const retryCaps = useAtomRefresh(capabilitiesAtom);
  const retryInfo = useAtomRefresh(serverInfoAtom);
  return (
    <ResultView onRetry={retryAll} result={allResult}>
      {(all) => (
        <ResultView onRetry={retryCaps} result={capsResult}>
          {(caps) => (
            <ResultView onRetry={retryInfo} result={infoResult}>
              {(info) => (
                <Explorer all={all} caps={caps} readOnly={info.readOnly} />
              )}
            </ResultView>
          )}
        </ResultView>
      )}
    </ResultView>
  );
};
