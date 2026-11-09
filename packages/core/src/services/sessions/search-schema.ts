/** Wire shapes for `sessions.search`. Node-free: the RPC contract imports it. */
import { Schema } from "effect";
import { AgentId } from "../agent-id";

/** One matching line of a transcript. */
export const SearchHit = Schema.Struct({
  /**
   * 1-based line in the loaded transcript text. For file-backed agents this is
   * the JSONL file line. OpenCode text is serialized by the reader, so the
   * number counts lines of that serialization, not of a file.
   */
  line: Schema.Number,
  /** Whitespace-collapsed text around the match, with `…` at cut edges. */
  snippet: Schema.String,
  /** Match range inside `snippet`; absent when redaction removed the query. */
  matchStart: Schema.optional(Schema.Number),
  matchEnd: Schema.optional(Schema.Number),
});
export type SearchHit = typeof SearchHit.Type;

/** One session with matches. */
export const SearchSession = Schema.Struct({
  id: Schema.String,
  agent: AgentId,
  project: Schema.String,
  title: Schema.optional(Schema.String),
  updatedAt: Schema.optional(Schema.String),
  source: Schema.optional(Schema.String),
  /** Every matching line in the transcript. */
  matchCount: Schema.Number,
  /** The first few distinct matches. */
  hits: Schema.Array(SearchHit),
});
export type SearchSession = typeof SearchSession.Type;

/** Sessions with matches, newest first. */
export const SessionSearchResult = Schema.Struct({
  sessions: Schema.Array(SearchSession),
  /** More sessions matched than `limit` allowed. */
  truncated: Schema.Boolean,
  /** Transcripts opened (may exceed the sessions returned). */
  scannedSessions: Schema.Number,
  /** Transcripts that could not be read. */
  unreadable: Schema.Number,
});
export type SessionSearchResult = typeof SessionSearchResult.Type;

/** `search` arguments. */
export interface SearchRequest {
  readonly agent?: AgentId | undefined;
  /** Max sessions returned, newest first. Default 20; 0 = all. */
  readonly limit?: number | undefined;
  readonly project?: string | undefined;
  /** Case-insensitive literal phrase. */
  readonly q: string;
  /** Redact secrets and home paths in snippet, title and project. Default true. */
  readonly redact?: boolean | undefined;
  /** ISO time: only sessions updated at or after it. */
  readonly since?: string | undefined;
}
