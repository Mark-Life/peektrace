/** One scanned folder: a label, the path with `~` for home, and a badge when
 * the folder is missing on disk. */
import { Badge } from "@workspace/ui/components/badge";
import { cn } from "@workspace/ui/lib/utils";
import type { ReactNode } from "react";
import { tildePath } from "../lib/paths";

export const RootRow = ({
  label,
  path,
  home,
  exists,
  className,
  testId,
  children,
}: {
  readonly label: string;
  readonly path: string;
  readonly home: string;
  readonly exists: boolean;
  readonly className?: string;
  readonly testId?: string;
  /** Shown between the path and the missing badge. */
  readonly children?: ReactNode;
}) => (
  <div
    className={cn(
      "flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-1",
      className
    )}
    data-testid={testId}
  >
    <span className="shrink-0 font-medium">{label}</span>
    <code className="min-w-0 break-all text-muted-foreground">
      {tildePath({ path, home })}
    </code>
    {children}
    {exists ? null : (
      <Badge data-testid="root-missing" variant="outline">
        not found
      </Badge>
    )}
  </div>
);
