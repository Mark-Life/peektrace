/** Session titles for chips outside the Sessions screen.
 *
 * Derived from the shared `sessions.list` atom, so Stats adds no request of its
 * own beyond keeping that list warm. An empty map while loading or failed makes
 * every chip fall back to `project/id`.
 */
import { Atom, Result } from "@effect-atom/atom-react";
import { sessionsListAtom } from "./atoms";
import { type SessionTitles, titleKey } from "./stats-format";

const NO_TITLES: SessionTitles = new Map();

/** `agent:id` to title, for every listed session that has one. */
export const sessionTitlesAtom = Atom.map(
  sessionsListAtom,
  (result): SessionTitles => {
    if (!Result.isSuccess(result)) {
      return NO_TITLES;
    }
    return new Map(
      result.value.flatMap((h) =>
        h.title ? [[titleKey(h.agent, h.id), h.title] as const] : []
      )
    );
  }
);
