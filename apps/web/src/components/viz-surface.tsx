import { Badge } from "@workspace/ui/components/badge";
import { cn } from "@workspace/ui/lib/utils";
import type { ReactNode } from "react";

interface VizSurfaceProps {
  /** Caption chip pinned to the top-right; states the data is a sample. */
  readonly caption?: string;
  readonly children: ReactNode;
  readonly className?: string;
  /** Mono eyebrow on the left of the header row. */
  readonly label?: string;
}

/**
 * Dark-scoped panel that hosts a real inspector visualization on the marketing
 * page. The charts are drawn against the inspector's dark palette (white
 * silhouettes, 400-weight zone text, GitHub-dark category hexes), so they are
 * pinned to the dark token set here rather than following the site theme — this
 * is the product surface, shown as it actually renders.
 */
export const VizSurface = ({
  caption,
  children,
  className,
  label,
}: VizSurfaceProps) => (
  <div
    className={cn(
      "dark flex flex-col gap-4 rounded-2xl border border-border bg-background p-4 text-foreground sm:p-6",
      className
    )}
  >
    {label || caption ? (
      <div className="flex flex-wrap items-center justify-between gap-2">
        {label ? (
          <span className="font-mono text-muted-foreground text-xs uppercase tracking-[0.2em]">
            {label}
          </span>
        ) : null}
        {caption ? (
          <Badge
            className="ml-auto font-mono text-[0.65rem] uppercase tracking-wider"
            variant="outline"
          >
            {caption}
          </Badge>
        ) : null}
      </div>
    ) : null}
    {children}
  </div>
);
