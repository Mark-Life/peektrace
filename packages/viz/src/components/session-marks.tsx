/** Small marks for a session: launcher → harness → model maker, and the
 * worktree marker. Logos draw through a CSS mask filled with `currentColor`,
 * monograms with theme tokens, so both read in light and dark themes. */
import { cn } from "@workspace/ui/lib/utils";
import type {
  Launcher,
  ModelProvider,
  Provider,
  Worktree,
} from "@workspace/view-core/api-types.gen";
import {
  harnessMark,
  launcherMark,
  providerMark,
  worktreeHint,
} from "@workspace/view-core/session-marks";
import type { Mark } from "@workspace/view-core/session-marks";
import { FolderGit2Icon } from "lucide-react";

import { PROVIDER_LOGO } from "../lib/provider-logos";

const MONOGRAM_CLASS =
  "border-border bg-muted text-muted-foreground inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-sm border px-0.5 text-[0.5625rem] leading-none font-semibold";

/** In forced-colors mode the browser would paint the mask fill with Canvas,
 * hiding the logo; it takes the system text colour instead. */
const LOGO_CLASS =
  "text-foreground/80 inline-block size-4 shrink-0 bg-current [mask-size:contain] [mask-position:center] [mask-repeat:no-repeat] forced-colors:bg-[color:CanvasText] forced-colors:[forced-color-adjust:none]";

/** The drawn part of a mark: the logo when `logo` is set, else the monogram.
 * Logos draw through a mask, so they take `currentColor`. */
const MarkGlyph = ({
  logo,
  text,
}: {
  readonly logo: string | undefined;
  readonly text: string;
}) =>
  logo === undefined ? (
    <span aria-hidden="true" className={MONOGRAM_CLASS}>
      {text}
    </span>
  ) : (
    <span
      aria-hidden="true"
      className={LOGO_CLASS}
      style={{ maskImage: `url("${logo}")` }}
    />
  );

/** One mark with its tooltip. Alone it is a labelled image; with `named` the
 * visible name beside it is the accessible text. */
const MarkView = ({
  logo,
  mark,
  name,
  named,
  testId,
}: {
  readonly logo: string | undefined;
  readonly mark: Mark;
  readonly name: string;
  readonly named: boolean;
  readonly testId: string;
}) =>
  named ? (
    <span
      className="inline-flex max-w-full min-w-0 items-center gap-1"
      data-testid={testId}
      title={mark.title}
    >
      <MarkGlyph logo={logo} text={mark.text} />
      <span className="truncate">{name}</span>
    </span>
  ) : (
    // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- a labelled mark, drawn by CSS
    <span
      aria-label={mark.label}
      className="inline-flex shrink-0"
      data-testid={testId}
      // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- a labelled mark, drawn by CSS
      role="img"
      title={mark.title}
    >
      <MarkGlyph logo={logo} text={mark.text} />
    </span>
  );

/** Launcher (when known) → harness → model maker (when a model is known). With
 * `named`, each mark shows its name too, for the session header. */
export const SessionMarks = ({
  className,
  harness,
  launcher,
  models,
  named = false,
  provider,
}: {
  readonly className?: string;
  readonly harness: Provider;
  readonly launcher: Launcher | undefined;
  /** Main model first. */
  readonly models: readonly string[];
  readonly named?: boolean;
  readonly provider: ModelProvider | undefined;
}) => {
  const run = harnessMark(harness);
  const maker = providerMark({ models, provider });

  return (
    <span
      className={cn(
        "items-center",
        // Named marks take the full width, so a long name wraps to its own
        // line and truncates there instead of leaving the card.
        named
          ? "flex min-w-0 flex-wrap gap-x-3 gap-y-1"
          : "inline-flex shrink-0 gap-1",
        className
      )}
      data-testid="session-marks"
    >
      {launcher === undefined ? null : (
        <MarkView
          logo={undefined}
          mark={launcherMark(launcher)}
          name={launcher.name}
          named={named}
          testId="session-launcher"
        />
      )}
      <MarkView
        logo={undefined}
        mark={run}
        name={run.label}
        named={named}
        testId="session-harness"
      />
      {maker === undefined ? null : (
        <MarkView
          logo={
            provider === undefined ? undefined : PROVIDER_LOGO.get(provider.id)
          }
          mark={maker}
          name={maker.label}
          named={named}
          testId="session-model-provider"
        />
      )}
    </span>
  );
};

/** Marks a session run in a linked worktree; the tooltip names its path and
 * the thread that started it. The accessible name stays short: screen readers
 * read it inside every row link, and the path is a long id. */
export const WorktreeMarker = ({
  parentTitle,
  worktree,
}: {
  readonly parentTitle: string | undefined;
  readonly worktree: Worktree;
}) => {
  const hint = worktreeHint({ parentTitle, worktree });

  return (
    // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- a labelled icon, not an image file
    <span
      aria-label="Worktree"
      className="text-muted-foreground inline-flex shrink-0"
      data-testid="session-worktree"
      // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role -- a labelled icon, not an image file
      role="img"
      title={hint}
    >
      <FolderGit2Icon aria-hidden="true" className="size-3.5" />
    </span>
  );
};
