/** Which reading of the history is on screen.
 *
 * Lives in the URL hash rather than the persisted per-session view, for the same
 * reason `?expand=all` does: it describes the whole section, not one row, so a
 * link to `…&view=chat` opens the way the sender was reading it. The table stays
 * the default — the chat is the readable pass, the table is the forensic one.
 */
import { setHashParam, useHashParam } from "./routes";

/** Dense forensics table, or the conversation rendering. */
export type TranscriptMode = "table" | "chat";

/** Reactively read the transcript layout from the hash (default `table`). */
export const useTranscriptMode = (): TranscriptMode =>
  useHashParam("view") === "chat" ? "chat" : "table";

/** Switch layouts; the default drops the param instead of spelling it out. */
export const setTranscriptMode = (mode: TranscriptMode) =>
  setHashParam("view", mode === "chat" ? "chat" : null);
