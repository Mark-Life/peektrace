import { Button } from "@workspace/ui/components/button";

/** Says a link or a summary jump points at something the search or type
 *  filter hides, with the way to show it. */
export const HiddenNote = ({
  hidden,
  onClear,
}: {
  readonly hidden: "call" | "jump" | false;
  readonly onClear: () => void;
}) =>
  hidden ? (
    <div
      className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs"
      data-testid={`history-${hidden}-hidden`}
    >
      <span>
        {hidden === "call"
          ? "The call this link points to is hidden by the search or type filter."
          : "The place you jumped to is hidden by the search or type filter."}
      </span>
      <Button
        className="h-6 px-2 text-xs"
        onClick={onClear}
        size="sm"
        variant="outline"
      >
        Clear filters
      </Button>
    </div>
  ) : null;
