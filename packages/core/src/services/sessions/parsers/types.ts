/** The per-agent parser contract used by `SessionsService` to dispatch ingest.
 *
 * Every coding agent stores transcripts in its own JSONL dialect (Claude,
 * Codex rollouts, Pi sessions). Each parser normalizes its dialect into the
 * shared `ParsedSession` / `SessionHeader` shapes so the analysis pipeline and
 * the UI stay agent-agnostic. Parsers are pure functions over transcript text;
 * all IO (file discovery, subagent folding) lives in the service.
 */
import type { AgentId } from "../../agent-id";
import type { ParsedSession, SessionHeader } from "../schema";

/** Inputs a parser needs to normalize one transcript into a `ParsedSession`. */
export interface ParseSessionArgs {
  /** Absolute path to the transcript. */
  readonly path: string;
  /** Session id resolved from the filename (or the transcript header). */
  readonly sessionId: string;
  /** Owning project slug, or `""` for layouts without one (Codex date tree). */
  readonly slug: string;
  /** Raw transcript text (whole `.jsonl` file). */
  readonly text: string;
}

/** Inputs a parser needs to build a lightweight list header (no full parse). */
export interface BuildHeaderArgs {
  readonly id: string;
  readonly mtimeMs: number;
  readonly path: string;
  readonly sizeBytes: number;
  readonly slug: string;
  readonly text: string;
}

/** Normalizes one agent's transcript dialect into the shared session shapes. */
export interface SessionParser {
  /** The agent this parser handles (drives the header `agent` badge/filter). */
  readonly agent: AgentId;
  /** Lightweight header scan for the list view, without building a timeline. */
  readonly buildHeader: (args: BuildHeaderArgs) => SessionHeader;
  /** Full parse of one transcript into the analysis-ready `ParsedSession`. */
  readonly parseSession: (args: ParseSessionArgs) => ParsedSession;
}
