/** Subagent (sidechain) transcript cards, below the history in either layout.
 *
 * A spawned agent runs in its own context window, so its tokens never count
 * against the session being inspected — the cards say so and stay a drill-down
 * rather than being folded into the transcript itself.
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { Badge } from "@workspace/ui/components/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@workspace/ui/components/collapsible";
import { fmt, fmtK } from "@workspace/viz/lib/session-format";
import { ChevronRightIcon } from "lucide-react";
import { subagentCollapseId } from "../../lib/session-view";

/** Subagent (sidechain) transcript cards — each runs in its own window. */
export const Subagents = ({
  a,
  isOpen,
  onToggle,
}: {
  readonly a: AnalyzedSession;
  readonly isOpen: (id: string) => boolean;
  readonly onToggle: (id: string, open: boolean) => void;
}) => {
  if (a.subagents.length === 0) {
    return null;
  }
  return (
    <div className="flex flex-col gap-2" data-testid="subagents">
      <h3 className="font-medium text-sm">Subagents ({a.subagents.length})</h3>
      <p className="text-muted-foreground text-xs">
        Each runs in its own context window — these tokens do not count against
        the main session.
      </p>
      {a.subagents.map((s) => (
        <Collapsible
          className="rounded-md border border-border"
          data-testid="subagent-card"
          key={s.id}
          onOpenChange={(o) => onToggle(subagentCollapseId(s.id), o)}
          open={isOpen(subagentCollapseId(s.id))}
        >
          <CollapsibleTrigger className="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-muted/40 [&[data-state=open]>svg]:rotate-90">
            <ChevronRightIcon className="size-3.5 shrink-0 text-muted-foreground transition-transform" />
            <Badge variant="secondary">{s.agentType ?? "agent"}</Badge>
            <span className="font-mono text-xs">{s.id}</span>
            <span className="truncate text-muted-foreground text-xs">
              {s.description ?? "subagent"} · {s.turns} turns
            </span>
            <span className="ml-auto shrink-0 font-mono text-muted-foreground text-xs">
              peak {fmtK(s.peakContextTokens)}
            </span>
          </CollapsibleTrigger>
          <CollapsibleContent>
            <pre className="whitespace-pre-wrap break-words bg-muted/30 px-3 py-2 text-xs">
              {`agentType: ${s.agentType ?? "—"}
description: ${s.description ?? "—"}
toolUseId: ${s.toolUseId ?? "—"}
turns: ${s.turns}
peak context: ${fmt(s.peakContextTokens)}
path: ${s.path}`}
            </pre>
          </CollapsibleContent>
        </Collapsible>
      ))}
    </div>
  );
};
