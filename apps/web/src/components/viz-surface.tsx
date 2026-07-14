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
 *
 * `min-w-0` is load-bearing: the charts wrap wide, nowrap content (the budget
 * table, the timeline SVG) whose min-content size propagates up through their
 * own scroll containers. As a flex or grid item the panel would otherwise take
 * `min-width: auto`, stretch its track to that intrinsic width, and push the
 * whole row off a phone screen instead of letting the inner charts scroll.
 */
export const VizSurface = ({
  caption,
  children,
  className,
  label,
}: VizSurfaceProps) => (
  <div
    className={cn(
      "dark flex min-w-0 flex-col gap-4 rounded-2xl border border-border bg-background p-4 text-foreground sm:p-6",
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
          // Badges are nowrap and `w-fit` by default; the caption is a sentence,
          // so it has to wrap rather than run past the panel on a phone.
          <Badge
            className="ml-auto h-auto max-w-full whitespace-normal font-mono text-[0.65rem] uppercase tracking-wider"
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
