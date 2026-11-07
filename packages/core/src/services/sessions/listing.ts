/** Session listing: one header per readable transcript, plus the transcripts
 * that failed to read, so a read error is counted instead of hidden. */
import type { FileSystem } from "@effect/platform";
import { Effect } from "effect";
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
      Effect.annotateCurrentSpan({
        unreadable: listing.unreadable.length,
        malformedLines: listing.headers.reduce(
          (sum, h) => sum + (h.malformedLines ?? 0),
          0
        ),
      })
    ),
    Effect.withSpan("Sessions.list", {
      attributes: filter?.agent ? { agent: filter.agent } : {},
    })
  );
};
