/** Skip links: the first Tab stops on the page, hidden until focused. */
import { cn } from "@workspace/ui/lib/utils";
import { hasInsetTitlebar } from "../lib/host";
import {
  focusLandmark,
  MAIN_CONTENT_ID,
  SESSION_DETAIL_ID,
} from "../lib/landmarks";
import { useSelectedSessionId } from "../lib/routes";

const SKIP_CLASS =
  "sr-only rounded-md bg-background px-3 py-2 font-medium text-sm shadow-md ring-2 ring-ring focus:not-sr-only focus:fixed focus:left-2 focus:z-50";

/** Focus the first of `targets` on the page. The session detail only exists
 *  once the list has loaded, so its link falls back to the main content. */
const focusFirst = (targets: readonly string[]) => {
  const found = targets.find((id) => document.getElementById(id) !== null);
  if (found !== undefined) {
    focusLandmark(found);
  }
};

const SkipLink = ({
  inset,
  label,
  targets,
}: {
  readonly inset: boolean;
  readonly label: string;
  readonly targets: readonly string[];
}) => (
  <button
    className={cn(
      SKIP_CLASS,
      // Clear the macOS window buttons that overlay the top-left corner.
      inset ? "focus:top-10" : "focus:top-2"
    )}
    data-testid="skip-link"
    onClick={() => focusFirst(targets)}
    type="button"
  >
    {label}
  </button>
);

/** "Skip to content", plus "Skip to session" while one is open. `inset` is
 *  true when native window buttons overlay the top of the page. */
export const SkipLinks = ({
  inset = false,
  sessionOpen,
}: {
  readonly inset?: boolean;
  readonly sessionOpen: boolean;
}) => (
  <>
    <SkipLink
      inset={inset}
      label="Skip to content"
      targets={[MAIN_CONTENT_ID]}
    />
    {sessionOpen ? (
      <SkipLink
        inset={inset}
        label="Skip to session"
        targets={[SESSION_DETAIL_ID, MAIN_CONTENT_ID]}
      />
    ) : null}
  </>
);

/** `SkipLinks` wired to the router and the host window. */
export const ShellSkipLinks = () => (
  <SkipLinks
    inset={hasInsetTitlebar()}
    sessionOpen={useSelectedSessionId() !== null}
  />
);
