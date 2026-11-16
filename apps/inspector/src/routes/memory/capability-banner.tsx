/** Read-only notice shown when memory editing is blocked.
 *
 * Every write affordance is hidden while the server is read-only or `memory.crud`
 * is not `supported`; this banner says why: the server mode, the matrix note, or
 * the per-agent support summary.
 */
import {
  Alert,
  AlertDescription,
  AlertTitle,
} from "@workspace/ui/components/alert";
import { LockIcon } from "lucide-react";

import { ReadOnlyReason } from "../../components/read-only";

/** Map a support level to a short verdict phrase. */
const VERDICT = new Map([
  ["partial", "partial"],
  ["planned", "planned"],
  ["supported", "supported"],
  ["unsupported", "not supported"],
]);

/** Render the blocked verdict. */
export const CapabilityBanner = ({
  blockedBy,
  level,
  detail,
}: {
  readonly blockedBy: "read-only" | "capability";
  readonly level: string;
  readonly detail: string;
}) => (
  <Alert data-testid="capability-banner">
    <LockIcon className="size-4" />
    {blockedBy === "read-only" ? (
      <>
        <AlertTitle>Read-only server</AlertTitle>
        <AlertDescription>
          <p>
            <ReadOnlyReason /> Memories can&apos;t be edited from here.
          </p>
        </AlertDescription>
      </>
    ) : (
      <>
        <AlertTitle>Memory editing is {VERDICT.get(level) ?? level}</AlertTitle>
        <AlertDescription>{detail}</AlertDescription>
      </>
    )}
  </Alert>
);
