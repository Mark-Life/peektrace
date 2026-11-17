/** A column heading whose definition opens on hover or focus, so the table
 * carries no paragraph of definitions above it. Its own provider lets a table
 * render outside the app shell. */
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@workspace/ui/components/tooltip";

/** A heading and its definition, as core writes them. */
interface Column {
  readonly label: string;
  readonly tip: string;
}

export const HeadTip = ({ column }: { readonly column: Column }) => (
  <TooltipProvider delayDuration={150}>
    <Tooltip>
      <TooltipTrigger asChild>
        {/* A button, so the definition opens from the keyboard too. */}
        <button
          className="cursor-help underline decoration-dotted underline-offset-2"
          type="button"
        >
          {column.label}
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-xs">{column.tip}</TooltipContent>
    </Tooltip>
  </TooltipProvider>
);
