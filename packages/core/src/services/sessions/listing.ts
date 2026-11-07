/** Session listing: one header per readable transcript, plus the transcripts
 * that failed to read, so a read error is counted instead of hidden. */
import type { FileSystem } from "@effect/platform";
import { Effect, Option, type Tracer } from "effect";
import { AGENT_IDS, type AgentId } from "../agent-id";
import type { AgentRegistryShape } from "../agents";
import {
  type HeaderCache,
  type HeaderResult,
  LIST_CONCURRENCY,
  listAgentHeaders,
} from "./header-cache";
import { PARSERS } from "./parsers";
import type {
  SessionHeader,
  SessionListing,
  UnreadableSession,
} from "./schema";

/** Options accepted by `list`. */
export interface ListFilter {
  readonly agent?: AgentId;
}

/** The agents that have a parser, in matrix order. */
export const supportedAgents = (
  agents: AgentRegistryShape
): readonly AgentId[] =>
  AGENT_IDS.filter((id) => agents.roots(id).supported && PARSERS[id]);

/** Root-span attribute that carries the unreadable count. */
export const UNREADABLE_ROOT_ATTR = "sessions.unreadable";

/** The nearest enclosing `peektrace.root` span, else the outermost span. */
const rootOf = (span: Tracer.Span): Tracer.Span => {
  if (span.attributes.get("peektrace.root") === true) {
    return span;
  }
  return Option.match(span.parent, {
    onNone: () => span,
    onSome: (p) => (p._tag === "Span" ? rootOf(p) : span),
  });
};

/**
 * Put a count on the root span too. A tracer may cap child spans per root, so a
 * listing with many transcripts can drop the `Sessions.list` span itself.
 */
const annotateRoot = (key: string, value: number) =>
  Effect.currentSpan.pipe(
    Effect.map((span) => rootOf(span).attribute(key, value)),
    Effect.ignore
  );

/** Split per-ref results into the listing. */
const toListing = (results: readonly HeaderResult[]): SessionListing => {
  const headers: SessionHeader[] = [];
  const unreadable: UnreadableSession[] = [];
  for (const r of results) {
    if (r._tag === "Header") {
      headers.push(r.header);
    } else if (r._tag === "Unreadable") {
      unreadable.push(r.session);
    }
  }
  return { headers, unreadable };
};

/** Headers for every supported agent's transcripts (or one agent's), plus the
 * unreadable ones. The span carries counts only: paths stay in the debug log. */
export const listSessions = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly cache: HeaderCache;
  readonly filter?: ListFilter;
}): Effect.Effect<SessionListing> => {
  const { fs, agents, cache, filter } = args;
  return Effect.forEach(
    supportedAgents(agents).filter(
      (agent) => !filter?.agent || agent === filter.agent
    ),
    (agent) => listAgentHeaders({ fs, agents, cache, agent }),
    { concurrency: LIST_CONCURRENCY }
  ).pipe(
    Effect.map((groups) => toListing(groups.flat())),
    Effect.tap((listing) =>
      Effect.zipRight(
        Effect.annotateCurrentSpan({
          unreadable: listing.unreadable.length,
          malformedLines: listing.headers.reduce(
            (sum, h) => sum + (h.malformedLines ?? 0),
            0
          ),
        }),
        annotateRoot(UNREADABLE_ROOT_ATTR, listing.unreadable.length)
      )
    ),
    Effect.withSpan("Sessions.list", {
      attributes: filter?.agent ? { agent: filter.agent } : {},
    })
  );
};
