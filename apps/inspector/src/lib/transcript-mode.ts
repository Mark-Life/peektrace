/** Which reading of the history is on screen.
 *
 * Lives in the URL hash rather than the persisted per-session view, for the same
 * reason `?expand=all` does: it describes the whole section, not one row, so a
 * link to `…&view=table` opens the way the sender was reading it. The chat is
 * the default — the readable pass; the table is the forensic one.
 */
import { setHashParam, useHashParam } from "./routes";

/** Dense forensics table, or the conversation rendering. */
export type TranscriptMode = "table" | "chat";

/** Parse the raw `view` param; anything but `table` reads as the default chat. */
export const transcriptModeOf = (raw: string | null): TranscriptMode =>
  raw === "table" ? "table" : "chat";

/** Reactively read the transcript layout from the hash (default `chat`). */
export const useTranscriptMode = () => transcriptModeOf(useHashParam("view"));

/** Switch layouts; the default drops the param instead of spelling it out. */
export const setTranscriptMode = (mode: TranscriptMode) =>
  setHashParam("view", mode === "table" ? "table" : null);
