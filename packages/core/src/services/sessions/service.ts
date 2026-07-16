/** SessionsService: lazy listing, full parse, and context-budget analysis.
 *
 * Multi-agent: `list` enumerates every supported agent's transcripts and builds
 * a header via that agent's parser; `parse`/`analyze` resolve a session id (or
 * path) to its owning agent and dispatch to the matching `SessionParser`.
 * Claude-only behaviours — subagent folding and on-disk CLAUDE.md/memory
 * attribution — are gated on `agent === "claude"`.
 */
import { FileSystem } from "@effect/platform";
import { Context, Effect, Layer } from "effect";
import { AGENT_IDS, type AgentId } from "../agent-id";
import {
  AgentRegistry,
  type AgentRegistryShape,
  type SessionFileRef,
} from "../agents";
import { analyze } from "./analyze";
import { SessionNotFoundError, TranscriptParseError } from "./errors";
import { parseClaudeSession } from "./parse";
import { PARSERS } from "./parsers";
import { redactParsed, redactSession } from "./redact";
import {
  findSubagents,
  gatherOnDiskContextFiles,
  resolveClaudeSession,
  type SubagentStub,
} from "./resolve";
import type {
  AnalyzedSession,
  ParsedSession,
  SessionHeader,
  SubagentRef,
} from "./schema";

const LIST_CONCURRENCY = 8;
const JSONL = /\.jsonl$/;

/** Options accepted by `parse`. */
export interface ParseRequest {
  readonly id: string;
  /** Redact secret-looking transcript text. Default true. */
  readonly redact?: boolean;
}

/** Options accepted by `analyze`. */
export interface AnalyzeRequest {
  /** Dumb-zone threshold as a fraction of the window. Default 0.40. */
  readonly dumbZone?: number;
  readonly id: string;
  /** Redact secret-looking transcript text. Default true. */
  readonly redact?: boolean;
  /** Explicit context-window override (tokens). Overrides the transcript. */
  readonly window?: number;
}

/** Service contract for multi-agent session ingest + analysis. */
export interface SessionsServiceShape {
  /** Reproduce the context-budget forensics for one transcript. */
  readonly analyze: (
    req: AnalyzeRequest
  ) => Effect.Effect<
    AnalyzedSession,
    SessionNotFoundError | TranscriptParseError
  >;
  /** Lightweight headers for every supported agent's transcripts. */
  readonly list: () => Effect.Effect<readonly SessionHeader[]>;
  /** Full parse of one transcript, folding in subagent transcripts (Claude). */
  readonly parse: (
    req: ParseRequest
  ) => Effect.Effect<
    ParsedSession,
    SessionNotFoundError | TranscriptParseError
  >;
}

/** Multi-agent session ingest + analysis. */
export class SessionsService extends Context.Tag("@peektrace/SessionsService")<
  SessionsService,
  SessionsServiceShape
>() {}

/** Parse one subagent file into a SubagentRef (its own window, sidechain on). */
const parseSubagent = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly stub: SubagentStub;
}): Effect.Effect<SubagentRef> => {
  const { fs, stub } = args;
  return fs.readFileString(stub.path).pipe(
    Effect.orElseSucceed(() => ""),
    Effect.map((text) => {
      const parsed = parseClaudeSession({
        text,
        path: stub.path,
        sessionId: stub.id,
        includeSidechainTurns: true,
      });
      const peakContextTokens = parsed.turns.reduce(
        (max, t) => Math.max(max, t.contextTokens),
        0
      );
      return {
        ...stub,
        turns: parsed.turns.length,
        peakContextTokens,
      } satisfies SubagentRef;
    })
  );
};

/** A resolved transcript: which agent owns it, and where it lives. */
interface Resolution extends SessionFileRef {
  readonly agent: AgentId;
}

/** The agents that have a parser, in matrix order. */
const supportedAgents = (agents: AgentRegistryShape): readonly AgentId[] =>
  AGENT_IDS.filter((id) => agents.roots(id).supported && PARSERS[id]);

/** Infer the owning agent of a direct `.jsonl` path from its containing root. */
const agentForPath = (
  agents: AgentRegistryShape,
  path: string
): AgentId | undefined =>
  supportedAgents(agents).find((id) =>
    path.startsWith(agents.roots(id).projectsRoot)
  );

/** Resolve a direct `.jsonl` path to its owning agent, or null if not one. */
const resolveDirectPath = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly idOrPath: string;
}): Effect.Effect<Resolution | null> => {
  const { fs, agents, idOrPath } = args;
  if (!idOrPath.endsWith(".jsonl")) {
    return Effect.succeed(null);
  }
  return fs.exists(idOrPath).pipe(
    Effect.orElseSucceed(() => false),
    Effect.map((exists) => {
      const agent = agentForPath(agents, idOrPath);
      if (!(exists && agent)) {
        return null;
      }
      const id = (idOrPath.split("/").pop() ?? idOrPath).replace(JSONL, "");
      return { agent, path: idOrPath, id, slug: "" } satisfies Resolution;
    })
  );
};

/**
 * Scan every supported agent's transcript list for an exact-id match, then fall
 * back to a unique short-id prefix across all agents. Null when unresolved.
 */
const scanForId = (args: {
  readonly agents: AgentRegistryShape;
  readonly wanted: string;
}): Effect.Effect<Resolution | null> => {
  const { agents, wanted } = args;
  return Effect.gen(function* () {
    const prefixMatches: Resolution[] = [];
    for (const agent of supportedAgents(agents)) {
      const refs = yield* agents.listSessionFiles(agent);
      const exact = refs.find((r) => r.id === wanted);
      if (exact) {
        return { agent, ...exact } satisfies Resolution;
      }
      for (const r of refs.filter((r) => r.id.startsWith(wanted))) {
        prefixMatches.push({ agent, ...r });
      }
    }
    const [match] = prefixMatches;
    return prefixMatches.length === 1 && match ? match : null;
  });
};

/**
 * Resolve an id (or direct `.jsonl` path) to its owning agent + file. Fast path
 * for a direct path; otherwise scan supported agents by id.
 */
const resolveSession = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly idOrPath: string;
}): Effect.Effect<Resolution, SessionNotFoundError> => {
  const { fs, agents, idOrPath } = args;
  return Effect.gen(function* () {
    const direct = yield* resolveDirectPath({ fs, agents, idOrPath });
    if (direct) {
      return direct;
    }
    const wanted = idOrPath.replace(JSONL, "");
    const found = yield* scanForId({ agents, wanted });
    if (found) {
      return found;
    }
    return yield* Effect.fail(
      new SessionNotFoundError({ id: wanted, searchedRoot: "all agents" })
    );
  }).pipe(Effect.withSpan("Sessions.resolve", { attributes: { idOrPath } }));
};

/** Fold Claude subagent transcripts into a parsed session (no-op elsewhere). */
const foldClaudeSubagents = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly parsed: ParsedSession;
  readonly id: string;
}): Effect.Effect<ParsedSession> => {
  const { fs, agents, parsed, id } = args;
  return resolveClaudeSession({ fs, agents, idOrPath: id }).pipe(
    Effect.flatMap((resolved) =>
      findSubagents({
        fs,
        ...(resolved.subagentDir ? { subagentDir: resolved.subagentDir } : {}),
      })
    ),
    Effect.flatMap((stubs) =>
      Effect.forEach(stubs, (stub) => parseSubagent({ fs, stub }), {
        concurrency: LIST_CONCURRENCY,
      })
    ),
    Effect.map(
      (subagents) => ({ ...parsed, subagents }) satisfies ParsedSession
    ),
    Effect.orElseSucceed(() => parsed)
  );
};

/** Resolve + full-parse one transcript via its agent's parser. */
const parseFull = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly id: string;
}): Effect.Effect<
  { readonly agent: AgentId; readonly parsed: ParsedSession },
  SessionNotFoundError | TranscriptParseError
> => {
  const { fs, agents, id } = args;
  return Effect.gen(function* () {
    const resolved = yield* resolveSession({ fs, agents, idOrPath: id });
    const parser = PARSERS[resolved.agent];
    if (!parser) {
      return yield* Effect.fail(
        new SessionNotFoundError({ id, searchedRoot: resolved.agent })
      );
    }
    const { text } = yield* agents
      .loadTranscript({ agent: resolved.agent, ref: resolved })
      .pipe(
        Effect.mapError(
          (e) => new TranscriptParseError({ path: e.path, reason: e.reason })
        )
      );
    const base = parser.parseSession({
      text,
      path: resolved.path,
      sessionId: resolved.id,
      slug: resolved.slug,
    });
    const parsed =
      resolved.agent === "claude"
        ? yield* foldClaudeSubagents({ fs, agents, parsed: base, id })
        : base;
    return { agent: resolved.agent, parsed };
  }).pipe(Effect.withSpan("Sessions.parse", { attributes: { id } }));
};

/** Build one header from a transcript ref via its agent's parser. */
const headerFor = (args: {
  readonly agents: AgentRegistryShape;
  readonly agent: AgentId;
  readonly ref: SessionFileRef;
}): Effect.Effect<SessionHeader | null> => {
  const { agents, agent, ref } = args;
  const parser = PARSERS[agent];
  if (!parser) {
    return Effect.succeed(null);
  }
  return agents.loadTranscript({ agent, ref }).pipe(
    Effect.map(({ text, sizeBytes, mtimeMs }) => {
      if (text === "" && sizeBytes === 0) {
        return null;
      }
      return parser.buildHeader({
        text,
        id: ref.id,
        slug: ref.slug,
        path: ref.path,
        sizeBytes,
        mtimeMs,
      });
    }),
    Effect.orElseSucceed(() => null)
  );
};

const makeService = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
}): SessionsServiceShape => {
  const { fs, agents } = args;

  const list: SessionsServiceShape["list"] = () =>
    Effect.forEach(
      supportedAgents(agents),
      (agent) =>
        agents.listSessionFiles(agent).pipe(
          Effect.flatMap((refs) =>
            Effect.forEach(refs, (ref) => headerFor({ agents, agent, ref }), {
              concurrency: LIST_CONCURRENCY,
            })
          )
        ),
      { concurrency: LIST_CONCURRENCY }
    ).pipe(
      Effect.map((groups) =>
        groups.flat().filter((h): h is SessionHeader => h !== null)
      ),
      Effect.withSpan("Sessions.list")
    );

  const parse: SessionsServiceShape["parse"] = ({ id, redact = true }) =>
    parseFull({ fs, agents, id }).pipe(
      Effect.map(({ parsed }) => (redact ? redactParsed(parsed) : parsed))
    );

  const analyzeReq: SessionsServiceShape["analyze"] = ({
    id,
    window,
    dumbZone,
    redact = true,
  }) =>
    Effect.gen(function* () {
      const { agent, parsed } = yield* parseFull({ fs, agents, id });
      const onDiskContextFiles =
        agent === "claude"
          ? yield* gatherOnDiskContextFiles({
              fs,
              agents,
              ...(parsed.cwd ? { cwd: parsed.cwd } : {}),
            })
          : [];
      const result = analyze(parsed, {
        onDiskContextFiles,
        ...(window === undefined ? {} : { window }),
        ...(dumbZone === undefined ? {} : { dumbZoneFraction: dumbZone }),
      });
      return redact ? redactSession(result) : result;
    }).pipe(Effect.withSpan("Sessions.analyze", { attributes: { id } }));

  return { list, parse, analyze: analyzeReq };
};

/** Live layer: depends on AgentRegistry + the platform FileSystem. */
export const SessionsServiceLive = Layer.effect(
  SessionsService,
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    const agents = yield* AgentRegistry;
    return makeService({ fs, agents });
  })
);
