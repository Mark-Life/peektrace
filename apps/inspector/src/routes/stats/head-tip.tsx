/** A column heading whose definition opens on hover or focus, so the table
 * carries no paragraph of definitions above it. Its own provider lets a table
 * render outside the app shell. */
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@workspace/ui/components/tooltip";

export const HeadTip = ({
  label,
  tip,
}: {
  readonly label: string;
  readonly tip: string;
}) => (
  <TooltipProvider delayDuration={150}>
    <Tooltip>
      <TooltipTrigger asChild>
        {/* A button, so the definition opens from the keyboard too. */}
        <button
          className="cursor-help underline decoration-dotted underline-offset-2"
          type="button"
        >
          {label}
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-xs">{tip}</TooltipContent>
    </Tooltip>
  </TooltipProvider>
);
