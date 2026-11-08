/** Skip links: the first Tab stops on the page, hidden until focused. */
import {
  focusLandmark,
  MAIN_CONTENT_ID,
  SESSION_DETAIL_ID,
} from "../lib/landmarks";
import { useSelectedSessionId } from "../lib/routes";

const SKIP_CLASS =
  "sr-only rounded-md bg-background px-3 py-2 font-medium text-sm shadow-md ring-2 ring-ring focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-50";

const SkipLink = ({
  label,
  target,
}: {
  readonly label: string;
  readonly target: string;
}) => (
  <button
    className={SKIP_CLASS}
    data-testid="skip-link"
    onClick={() => focusLandmark(target)}
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
    <SkipLink label="Skip to content" target={MAIN_CONTENT_ID} />
    {sessionOpen ? (
      <SkipLink label="Skip to session" target={SESSION_DETAIL_ID} />
    ) : null}
  </>
);

/** `SkipLinks` wired to the router. */
export const ShellSkipLinks = () => (
  <SkipLinks sessionOpen={useSelectedSessionId() !== null} />
);
