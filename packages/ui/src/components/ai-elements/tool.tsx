"use client";

/** Tool call / tool result presentation, vendored from Vercel's AI Elements.
 *
 * Upstream: https://elements.ai-sdk.dev/api/registry/tool.json. It diverges here
 * in the ways a *finished transcript* forces, since we render `AnalyzedSession`
 * events rather than a live AI SDK stream:
 *
 *  - `ToolState` replaces `ToolUIPart["state"]`. A recorded call is never
 *    "streaming" or "awaiting approval" — it either got a result, got a failed
 *    result, or never got one at all.
 *  - `ToolInput`/`ToolOutput` take `code` + `language` instead of an `unknown`
 *    payload: the shared highlighter needs an explicit language, and callers
 *    already unwrap JSON-encoded tool payloads into their real shape.
 *  - `ToolHeader` takes `icon`, `lead` and `children` slots, so a dense forensics
 *    list can hang turn numbers, badges and token counts off the same trigger row.
 *  - No `mb-4` on the root and no padding on `ToolContent`: spacing belongs to
 *    whatever stacks these, and each pane owns its own, so a payload can run
 *    edge to edge under its label at any density.
 */

import { Badge } from "@workspace/ui/components/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@workspace/ui/components/collapsible";
import type { CodeBlockLanguage } from "@workspace/ui/lib/highlighter";
import { cn } from "@workspace/ui/lib/utils";
import {
  CheckCircleIcon,
  ChevronRightIcon,
  CircleDashedIcon,
  WrenchIcon,
  XCircleIcon,
} from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { CodeBlock, CodeBlockCopyButton } from "./code-block";

/** What a recorded tool call ended up doing. */
export type ToolState = "completed" | "error" | "unanswered";

export type ToolProps = ComponentProps<typeof Collapsible>;

export const Tool = ({ className, ...props }: ToolProps) => (
  <Collapsible
    className={cn("group not-prose w-full rounded-md border", className)}
    {...props}
  />
);

const STATUS_LABELS: Record<ToolState, string> = {
  completed: "completed",
  error: "error",
  unanswered: "no result",
};

const STATUS_ICONS: Record<ToolState, ReactNode> = {
  completed: <CheckCircleIcon className="size-3 text-emerald-500" />,
  error: <XCircleIcon className="size-3" />,
  unanswered: <CircleDashedIcon className="size-3" />,
};

export type ToolStatusBadgeProps = Omit<
  ComponentProps<typeof Badge>,
  "children"
> & {
  readonly state: ToolState;
};

/** Icon + label for a call's outcome; errors take the destructive variant. */
export const ToolStatusBadge = ({
  className,
  state,
  ...props
}: ToolStatusBadgeProps) => (
  <Badge
    className={cn("shrink-0 gap-1 rounded-full text-xs", className)}
    variant={state === "error" ? "destructive" : "secondary"}
    {...props}
  >
    {STATUS_ICONS[state]}
    {STATUS_LABELS[state]}
  </Badge>
);

export type ToolHeaderProps = ComponentProps<typeof CollapsibleTrigger> & {
  /** Replaces the wrench, e.g. to tell an invocation from what it returned. */
  readonly icon?: ReactNode;
  /** Rendered before the icon, e.g. a turn-number gutter. */
  readonly lead?: ReactNode;
  readonly name: string;
  readonly state: ToolState;
};

export const ToolHeader = ({
  children,
  className,
  icon,
  lead,
  name,
  state,
  ...props
}: ToolHeaderProps) => (
  <CollapsibleTrigger
    className={cn("flex w-full items-center gap-2 p-3 text-left", className)}
    {...props}
  >
    <ChevronRightIcon className="size-3.5 shrink-0 text-muted-foreground transition-transform group-data-[state=open]:rotate-90" />
    {lead}
    {icon ?? <WrenchIcon className="size-3.5 shrink-0 text-muted-foreground" />}
    <span className="shrink-0 font-medium font-mono text-sm">{name}</span>
    <ToolStatusBadge state={state} />
    {children}
  </CollapsibleTrigger>
);

export type ToolContentProps = ComponentProps<typeof CollapsibleContent>;

export const ToolContent = ({ className, ...props }: ToolContentProps) => (
  <CollapsibleContent
    className={cn(
      "text-popover-foreground outline-none",
      "data-[state=closed]:fade-out-0 data-[state=closed]:slide-out-to-top-2 data-[state=open]:slide-in-from-top-2 data-[state=closed]:animate-out data-[state=open]:animate-in",
      className
    )}
    {...props}
  />
);

/** Section heading shared by the parameters and result panes. */
const ToolSectionLabel = ({
  children,
  isError = false,
}: {
  readonly children: ReactNode;
  readonly isError?: boolean;
}) => (
  <h4
    className={cn(
      "px-3 pt-2 font-medium text-xs uppercase tracking-wide",
      isError ? "text-destructive" : "text-muted-foreground"
    )}
  >
    {children}
  </h4>
);

/** A copyable, highlighted payload pane. */
const ToolPayload = ({
  code,
  language,
}: {
  readonly code: string;
  readonly language: CodeBlockLanguage;
}) => (
  <CodeBlock
    className="[&_pre]:whitespace-pre-wrap! [&_pre]:wrap-break-word! rounded-none border-0 border-t [&_code]:text-[11px]! [&_pre]:p-3! [&_pre]:text-[11px]! [&_pre]:leading-relaxed!"
    code={code}
    language={language}
  >
    <CodeBlockCopyButton className="absolute top-2 right-2 z-10" />
  </CodeBlock>
);

export type ToolInputProps = ComponentProps<"div"> & {
  readonly code: string;
  readonly language: CodeBlockLanguage;
  /** Overrides the "Parameters" heading, e.g. with the unwrapped arg's name. */
  readonly label?: string;
};

export const ToolInput = ({
  className,
  code,
  label = "Parameters",
  language,
  ...props
}: ToolInputProps) => (
  <div className={cn("overflow-hidden", className)} {...props}>
    <ToolSectionLabel>{label}</ToolSectionLabel>
    <ToolPayload code={code} language={language} />
  </div>
);

export type ToolOutputProps = ComponentProps<"div"> & {
  readonly code: string;
  readonly isError?: boolean;
  readonly language: CodeBlockLanguage;
  readonly label?: string;
};

export const ToolOutput = ({
  className,
  code,
  isError = false,
  label,
  language,
  ...props
}: ToolOutputProps) => (
  <div
    className={cn(
      "overflow-hidden",
      isError && "bg-destructive/10",
      className
    )}
    {...props}
  >
    <ToolSectionLabel isError={isError}>
      {label ?? (isError ? "Error" : "Result")}
    </ToolSectionLabel>
    <ToolPayload code={code} language={language} />
  </div>
);
