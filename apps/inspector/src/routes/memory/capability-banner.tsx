/** Capability verdict banner shown when the selected agent can't edit memory.
 *
 * Drives Phase 7.6: for any non-Claude agent the `memory.crud` matrix cell is
 * not `supported`, so every write affordance is hidden and this banner shows the
 * verdict + note (e.g. "Codex memory: planned"). Claude (supported) shows
 * nothing.
 */
import type { AgentId } from "@workspace/core/services/agent-id";
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@workspace/ui/components/alert";
import { LockIcon } from "lucide-react";

/** Map a support level to a short verdict phrase. */
const VERDICT: Record<string, string> = {
  planned: "planned",
  partial: "partial",
  unsupported: "not supported",
  supported: "supported",
};

/** Render the read-only verdict for a gated agent. */
export const CapabilityBanner = ({
  agent,
  level,
  note,
}: {
  readonly agent: AgentId;
  readonly level: string;
  readonly note?: string;
}) => (
  <Alert className="mb-4" data-testid="capability-banner">
    <LockIcon className="size-4" />
    <AlertTitle className="capitalize">
      {agent} memory editing is {VERDICT[level] ?? level}
    </AlertTitle>
    <AlertDescription>
      {note ??
        `The capability matrix marks memory.crud as "${level}" for ${agent}; editing is read-only here. Claude is the only agent with markdown memory CRUD today.`}
    </AlertDescription>
  </Alert>
);
