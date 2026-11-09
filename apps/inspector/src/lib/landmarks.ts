/** Ids of the places skip links and shortcuts move focus to. */

export const MAIN_CONTENT_ID = "main-content";
export const SESSION_DETAIL_ID = "session-detail";
export const HISTORY_SEARCH_ID = "history-search";

/** Focus a landmark by id. Skip links are buttons, not `#id` anchors: the
 *  hash is the router, and an anchor would replace the open session. */
export const focusLandmark = (id: string) => {
  const el = document.getElementById(id);
  el?.focus({ preventScroll: true });
  el?.scrollIntoView({ block: "start" });
};
