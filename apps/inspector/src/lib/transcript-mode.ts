/** Which reading of the history is on screen.
 *
 * The stored history prefs carry the user's chosen layout across sessions. A
 * `view` param in the URL hash overrides it, so a link to `…&view=table` opens
 * the way the sender was reading it. With neither, the chat is the default —
 * the readable pass; the table is the forensic one.
 */

/** Dense forensics table, or the conversation rendering. */
export type TranscriptMode = "table" | "chat";

/** Layout used when nothing is stored and the hash names none. */
export const DEFAULT_TRANSCRIPT_MODE: TranscriptMode = "chat";

/** Parse an untrusted mode value; anything unknown reads as `null`. */
export const transcriptModeOf = (raw: unknown): TranscriptMode | null =>
  raw === "table" || raw === "chat" ? raw : null;

/** An explicit hash `view` wins over the stored layout. */
export const resolveTranscriptMode = ({
  hash,
  stored,
}: {
  readonly hash: string | null;
  readonly stored: TranscriptMode;
}) => transcriptModeOf(hash) ?? stored;
