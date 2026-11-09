/** Project field for the create dialog, plus the page-level "New memory in…".
 *
 * The picker reads the session list only once the dialog is open, so the
 * Memory page itself never waits on it. Sessions only add recency order;
 * the server's project labels fill the list before they arrive.
 */
import { Result, useAtomValue } from "@effect-atom/atom-react";
import type { ProjectLabel } from "@workspace/core/services/memory/types";
import { Button } from "@workspace/ui/components/button";
import { Label } from "@workspace/ui/components/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select";
import { PlusIcon } from "lucide-react";
import { useMemo } from "react";
import { sessionsListAtom } from "../../lib/atoms";
import { memoryProjectOptions, type ProjectOption } from "./project-options";

const FIELD_ID = "mem-project";

/** Trigger text for the loading, empty and ready states. */
const placeholderFor = ({
  loading,
  count,
}: {
  readonly loading: boolean;
  readonly count: number;
}) => {
  if (loading) {
    return "Loading projects…";
  }
  return count === 0 ? "No Claude projects found" : "Choose a project";
};

/** Labelled project select over Claude project dirs, newest session first. */
export const ProjectPicker = ({
  labels,
  value,
  onChange,
}: {
  readonly labels: readonly ProjectLabel[];
  readonly value: ProjectOption | undefined;
  readonly onChange: (option: ProjectOption) => void;
}) => {
  const result = useAtomValue(sessionsListAtom);
  const headers = Result.isSuccess(result) ? result.value : undefined;
  const options = useMemo(
    () => memoryProjectOptions({ headers: headers ?? [], labels }),
    [headers, labels]
  );
  const loading =
    options.length === 0 && headers === undefined && !Result.isFailure(result);

  const placeholder = placeholderFor({ loading, count: options.length });

  return (
    <div className="grid gap-1.5">
      <Label htmlFor={FIELD_ID}>Project</Label>
      <Select
        disabled={options.length === 0}
        onValueChange={(slug) => {
          const picked = options.find((o) => o.slug === slug);
          if (picked) {
            onChange(picked);
          }
        }}
        value={value?.slug ?? ""}
      >
        <SelectTrigger
          className="w-full min-w-0"
          data-testid="create-project"
          id={FIELD_ID}
        >
          <SelectValue placeholder={placeholder} />
        </SelectTrigger>
        <SelectContent>
          {options.map((o) => (
            <SelectItem key={o.slug} value={o.slug}>
              {o.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      {Result.isFailure(result) && options.length === 0 ? (
        <p className="text-red-400 text-xs" data-testid="create-project-error">
          Could not load the session list.
        </p>
      ) : null}
      {Result.isSuccess(result) && options.length === 0 ? (
        <p
          className="text-muted-foreground text-xs"
          data-testid="create-project-none"
        >
          Memory needs a Claude project. Start a Claude session in a project
          first.
        </p>
      ) : null}
    </div>
  );
};

/** The "New memory in…" trigger button. */
export const CreateAnywhereButton = ({
  onClick,
}: {
  readonly onClick: () => void;
}) => (
  <Button data-testid="create-anywhere" onClick={onClick} size="sm">
    <PlusIcon className="size-4" /> New memory in…
  </Button>
);
