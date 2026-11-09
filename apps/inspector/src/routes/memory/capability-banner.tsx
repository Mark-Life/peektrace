/** Read-only notice shown when the capability matrix blocks memory editing.
 *
 * Every write affordance is hidden while `memory.crud` is not `supported`; this
 * banner says why: the matrix note, or the per-agent support summary.
 */
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

/** Render the read-only verdict. */
export const CapabilityBanner = ({
  level,
  detail,
}: {
  readonly level: string;
  readonly detail: string;
}) => (
  <Alert data-testid="capability-banner">
    <LockIcon className="size-4" />
    <AlertTitle>Memory editing is {VERDICT[level] ?? level}</AlertTitle>
    <AlertDescription>{detail}</AlertDescription>
  </Alert>
);
