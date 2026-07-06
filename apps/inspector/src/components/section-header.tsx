/** Reusable page heading: title + one-line description. */
import type { ReactNode } from "react";

/** Render a section title with an optional trailing action slot. */
export const SectionHeader = ({
  title,
  description,
  action,
}: {
  readonly title: string;
  readonly description: string;
  readonly action?: ReactNode;
}) => (
  <header className="mb-6 flex items-start justify-between gap-4">
    <div className="flex flex-col gap-1">
      <h1 className="font-semibold text-2xl tracking-tight">{title}</h1>
      <p className="text-muted-foreground text-sm">{description}</p>
    </div>
    {action}
  </header>
);
