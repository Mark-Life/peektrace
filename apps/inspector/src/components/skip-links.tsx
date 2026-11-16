/** Skip links: the first Tab stops on the page, hidden until focused. */
import {
  focusLandmark,
  MAIN_CONTENT_ID,
  SESSION_DETAIL_ID,
} from "../lib/landmarks";
import { useSelectedSessionId } from "../lib/routes";

const SKIP_CLASS =
  "sr-only rounded-md bg-background font-medium text-sm shadow-md ring-2 ring-ring focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-50 focus:px-3 focus:py-2";

/** Focus the first of `targets` on the page. The session detail only exists
 *  once the list has loaded, so its link falls back to the main content. */
const focusFirst = (targets: readonly string[]) => {
  const found = targets.find((id) => document.getElementById(id) !== null);
  if (found !== undefined) {
    focusLandmark(found);
  }
};

const SkipLink = ({
  label,
  targets,
}: {
  readonly label: string;
  readonly targets: readonly string[];
}) => (
  <button
    className={SKIP_CLASS}
    data-testid="skip-link"
    onClick={() => focusFirst(targets)}
    type="button"
  >
    {label}
  </button>
);

/** "Skip to content", plus "Skip to session" while one is open. */
export const SkipLinks = ({
  sessionOpen,
}: {
  readonly sessionOpen: boolean;
}) => (
  <>
    <SkipLink label="Skip to content" targets={[MAIN_CONTENT_ID]} />
    {sessionOpen ? (
      <SkipLink
        label="Skip to session"
        targets={[SESSION_DETAIL_ID, MAIN_CONTENT_ID]}
      />
    ) : null}
  </>
);

/** `SkipLinks` wired to the router. */
export const ShellSkipLinks = () => (
  <SkipLinks sessionOpen={useSelectedSessionId() !== null} />
);
