/** Which transcript the open history reads from.
 *
 * `AnalyzedSession` carries no id, but an image is fetched from its transcript
 * line by line number, so the rows deep in the history need the session id (and
 * the parent, for a subagent) without threading it through every component.
 */
import { createContext, useContext } from "react";

export interface TranscriptSource {
  readonly id: string;
  /** Set when `id` is a subagent transcript, resolved through this session. */
  readonly parent: string | null;
}

export const TranscriptSourceContext = createContext<TranscriptSource | null>(
  null
);

/** The open transcript, or null outside a session view. */
export const useTranscriptSource = () => useContext(TranscriptSourceContext);
