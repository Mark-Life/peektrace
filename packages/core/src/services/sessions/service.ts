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
import type { AgentId } from "../agent-id";
import {
  AgentRegistry,
  type AgentRegistryShape,
  type SessionFileRef,
  sourcesOf,
} from "../agents";
import { analyze } from "./analyze";
import { SessionNotFoundError, TranscriptParseError } from "./errors";
import {
  type HeaderCache,
  LIST_CONCURRENCY,
  makeHeaderCache,
} from "./header-cache";
import { imageAt } from "./images";
import { type ListFilter, listSessions, supportedAgents } from "./listing";
import { jsonlLineAt, parseClaudeSession, parseJsonl } from "./parse";
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
  SessionListing,
  SubagentRef,
} from "./schema";
import {
  pickWorkflowCall,
  type WorkflowCall,
  workflowCallsByRun,
} from "./workflows";

const JSONL = /\.jsonl$/;

/** Options accepted by `parse`. */
export interface ParseRequest {
  readonly id: string;
  /** Parent session id when `id` names a nested (subagent) transcript. */
  readonly parent?: string;
  /** Redact secret-looking transcript text. Default true. */
  readonly redact?: boolean;
}

export type { ListFilter } from "./listing";

/** Options accepted by `image`. */
export interface ImageRequest {
  readonly block: number;
  readonly id: string;
  readonly item?: number;
  /** Line of the event in the parsed transcript (`TimelineEvent.index`). */
  readonly line: number;
  readonly parent?: string;
}

/** Options accepted by `analyze`. */
export interface AnalyzeRequest {
  /** Dumb-zone threshold as a fraction of the window. Default 0.40. */
  readonly dumbZone?: number;
  readonly id: string;
  /** Parent session id when `id` names a nested (subagent) transcript. */
  readonly parent?: string;
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
  /**
   * One image's bytes from a Claude transcript, located by the event's line and
   * `ImageRef`; null when the ref names no showable image.
   */
  readonly image: (
    req: ImageRequest
  ) => Effect.Effect<
    NonNullable<ReturnType<typeof imageAt>> | null,
    SessionNotFoundError | TranscriptParseError
  >;
  /**
   * Lightweight headers for every supported agent's transcripts, or one
   * agent's when `filter.agent` is set, plus the transcripts that could not
   * be read. Unchanged files reuse cached headers.
   */
  readonly list: (filter?: ListFilter) => Effect.Effect<SessionListing>;
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

/**
 * Parse one subagent file into a SubagentRef (its own window, sidechain on).
 * `link` sets the Workflow call once the transcript start time is known.
 */
const parseSubagent = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly stub: SubagentStub;
  readonly link: ReturnType<typeof workflowLinker>;
}): Effect.Effect<SubagentRef> => {
  const { fs, stub, link } = args;
  return fs.readFileString(stub.path).pipe(
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
        ...link(stub, parsed.startedAt),
        turns: parsed.turns.length,
        peakContextTokens,
      } satisfies SubagentRef;
    }),
    Effect.catchAll((e) =>
      Effect.logDebug("subagent unreadable").pipe(
        Effect.annotateLogs({ path: stub.path, reason: e.message }),
        Effect.as<SubagentRef>({
          ...link(stub),
          turns: 0,
          peakContextTokens: 0,
          unreadable: true,
        })
      )
    )
  );
};

/** A resolved transcript: which agent owns it, and where it lives. */
interface Resolution extends SessionFileRef {
  readonly agent: AgentId;
}

/** Infer the owning agent of a direct `.jsonl` path from its containing root
 * (any of the agent's sources, so a second account's path resolves too). */
const agentForPath = (
  agents: AgentRegistryShape,
  path: string
): AgentId | undefined =>
  supportedAgents(agents).find((id) =>
    sourcesOf(agents.roots(id)).some((s) => path.startsWith(s.projectsRoot))
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

/**
 * Build a linker that points each workflow agent's `toolUseId` at the Workflow
 * call that started its run, or the resume call that was live when it started.
 * The start time comes from the run json, else from the agent's own transcript.
 * Runs with no call in this transcript stay unlinked.
 */
const workflowLinker = (args: {
  readonly stubs: readonly SubagentStub[];
  readonly text: string;
}) => {
  const { stubs, text } = args;
  const calls = stubs.some((s) => s.workflowRunId && !s.toolUseId)
    ? workflowCallsByRun(text)
    : new Map<string, readonly WorkflowCall[]>();
  return (
    { workflowStartedAt, ...stub }: SubagentStub,
    transcriptStartedAt?: string
  ) => {
    if (!stub.workflowRunId || stub.toolUseId) {
      return stub;
    }
    const fallback = transcriptStartedAt
      ? Date.parse(transcriptStartedAt)
      : Number.NaN;
    const call = pickWorkflowCall(
      calls.get(stub.workflowRunId) ?? [],
      workflowStartedAt ?? (Number.isNaN(fallback) ? undefined : fallback)
    );
    return call ? { ...stub, toolUseId: call } : stub;
  };
};

/** Fold Claude subagent transcripts into a parsed session (no-op elsewhere). */
const foldClaudeSubagents = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly parsed: ParsedSession;
  readonly id: string;
  /** Raw main transcript, scanned for the Workflow calls that started runs. */
  readonly text: string;
}): Effect.Effect<ParsedSession> => {
  const { fs, agents, parsed, id, text } = args;
  return resolveClaudeSession({ fs, agents, idOrPath: id }).pipe(
    Effect.flatMap((resolved) =>
      findSubagents({
        fs,
        ...(resolved.subagentDir ? { subagentDir: resolved.subagentDir } : {}),
      })
    ),
    Effect.flatMap((stubs) => {
      const link = workflowLinker({ stubs, text });
      return Effect.forEach(
        stubs,
        (stub) => parseSubagent({ fs, stub, link }),
        {
          concurrency: LIST_CONCURRENCY,
        }
      );
    }),
    Effect.map(
      (subagents) => ({ ...parsed, subagents }) satisfies ParsedSession
    ),
    Effect.catchAll((e) =>
      Effect.logDebug("subagents not folded").pipe(
        Effect.annotateLogs({ id, reason: e._tag }),
        Effect.as(parsed)
      )
    )
  );
};

/** What a full parse yields: the owning agent, the session, its source. */
interface FullParse {
  readonly agent: AgentId;
  readonly parsed: ParsedSession;
  readonly source?: string;
}

/** One transcript's text, located by id (and parent, for a nested one). */
interface LoadedTranscript {
  readonly agent: AgentId;
  readonly id: string;
  /** Nested under a parent session (a Claude subagent file). */
  readonly nested: boolean;
  readonly path: string;
  readonly slug: string;
  readonly source?: string;
  readonly text: string;
}

const toParseError = Effect.mapError(
  (e: { readonly path: string; readonly reason: string }) =>
    new TranscriptParseError({ path: e.path, reason: e.reason })
);

/**
 * Locate + read a nested transcript through its parent. Only Claude nests
 * subagents under a session dir, so the parent bounds the search to one
 * `subagents` tree instead of a corpus walk.
 */
const loadNested = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly id: string;
  readonly parent: string;
}): Effect.Effect<
  LoadedTranscript,
  SessionNotFoundError | TranscriptParseError
> => {
  const { fs, agents, id, parent } = args;
  const notFound = new SessionNotFoundError({
    id,
    searchedRoot: `subagents of ${parent}`,
  });
  return Effect.gen(function* () {
    const owner = yield* resolveSession({ fs, agents, idOrPath: parent });
    if (owner.agent !== "claude") {
      return yield* Effect.fail(notFound);
    }
    const { subagentDir } = yield* resolveClaudeSession({
      fs,
      agents,
      idOrPath: owner.path,
    });
    const stubs = yield* findSubagents({
      fs,
      ...(subagentDir ? { subagentDir } : {}),
    });
    // Exact id, else a prefix that names exactly one subagent (as printed).
    const prefixed = stubs.filter((s) => s.id.startsWith(id));
    const stub =
      stubs.find((s) => s.id === id) ??
      (prefixed.length === 1 ? prefixed[0] : undefined);
    if (!stub) {
      return yield* Effect.fail(notFound);
    }
    const ref: SessionFileRef = {
      id: stub.id,
      path: stub.path,
      slug: owner.slug,
      ...(owner.source ? { source: owner.source } : {}),
    };
    const { text } = yield* agents
      .loadTranscript({ agent: "claude", ref })
      .pipe(toParseError);
    return {
      agent: owner.agent,
      id: stub.id,
      nested: true,
      path: stub.path,
      slug: owner.slug,
      text,
      ...(owner.source ? { source: owner.source.id } : {}),
    } satisfies LoadedTranscript;
  });
};

/** Locate + read one transcript, nested through `parent` when given. */
const loadText = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly id: string;
  readonly parent?: string | undefined;
}): Effect.Effect<
  LoadedTranscript,
  SessionNotFoundError | TranscriptParseError
> => {
  const { fs, agents, id, parent } = args;
  if (parent !== undefined) {
    return loadNested({ fs, agents, id, parent });
  }
  return Effect.gen(function* () {
    const resolved = yield* resolveSession({ fs, agents, idOrPath: id });
    const { text } = yield* agents
      .loadTranscript({ agent: resolved.agent, ref: resolved })
      .pipe(toParseError);
    return {
      agent: resolved.agent,
      id: resolved.id,
      nested: false,
      path: resolved.path,
      slug: resolved.slug,
      text,
      ...(resolved.source ? { source: resolved.source.id } : {}),
    } satisfies LoadedTranscript;
  });
};

/** Resolve + full-parse one transcript via its agent's parser. */
const parseFull = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly id: string;
  readonly parent?: string | undefined;
}): Effect.Effect<FullParse, SessionNotFoundError | TranscriptParseError> => {
  const { fs, agents, id, parent } = args;
  return Effect.gen(function* () {
    const loaded = yield* loadText({ fs, agents, id, parent });
    const source = loaded.source ? { source: loaded.source } : {};
    if (loaded.nested) {
      const parsed = parseClaudeSession({
        text: loaded.text,
        path: loaded.path,
        sessionId: loaded.id,
        includeSidechainTurns: true,
      });
      return { agent: loaded.agent, parsed, ...source } satisfies FullParse;
    }
    const parser = PARSERS[loaded.agent];
    if (!parser) {
      return yield* Effect.fail(
        new SessionNotFoundError({ id, searchedRoot: loaded.agent })
      );
    }
    const base = parser.parseSession({
      text: loaded.text,
      path: loaded.path,
      sessionId: loaded.id,
      slug: loaded.slug,
    });
    const parsed =
      loaded.agent === "claude"
        ? yield* foldClaudeSubagents({
            fs,
            agents,
            parsed: base,
            id,
            text: loaded.text,
          })
        : base;
    return { agent: loaded.agent, parsed, ...source } satisfies FullParse;
  }).pipe(
    Effect.withSpan(
      parent === undefined ? "Sessions.parse" : "Sessions.parseNested",
      {
        attributes: { id, ...(parent === undefined ? {} : { parent }) },
      }
    )
  );
};

const makeService = (args: {
  readonly fs: FileSystem.FileSystem;
  readonly agents: AgentRegistryShape;
  readonly cache: HeaderCache;
}): SessionsServiceShape => {
  const { fs, agents, cache } = args;

  const list: SessionsServiceShape["list"] = (filter) =>
    listSessions({ fs, agents, cache, ...(filter ? { filter } : {}) });

  const parse: SessionsServiceShape["parse"] = ({
    id,
    parent,
    redact = true,
  }) =>
    parseFull({ fs, agents, id, parent }).pipe(
      Effect.map(({ parsed }) => (redact ? redactParsed(parsed) : parsed))
    );

  const analyzeReq: SessionsServiceShape["analyze"] = ({
    id,
    parent,
    window,
    dumbZone,
    redact = true,
  }) =>
    Effect.gen(function* () {
      const { agent, parsed, source } = yield* parseFull({
        fs,
        agents,
        id,
        parent,
      });
      const onDiskContextFiles =
        agent === "claude"
          ? yield* gatherOnDiskContextFiles({
              fs,
              agents,
              ...(parsed.cwd ? { cwd: parsed.cwd } : {}),
              ...(source ? { source } : {}),
            })
          : [];
      const result = analyze(parsed, {
        onDiskContextFiles,
        ...(window === undefined ? {} : { window }),
        ...(dumbZone === undefined ? {} : { dumbZoneFraction: dumbZone }),
      });
      return redact ? redactSession(result) : result;
    }).pipe(Effect.withSpan("Sessions.analyze", { attributes: { id } }));

  const image: SessionsServiceShape["image"] = ({
    id,
    parent,
    line,
    block,
    item,
  }) =>
    loadText({ fs, agents, id, parent }).pipe(
      Effect.map(({ agent, text }) => {
        if (agent !== "claude") {
          return null;
        }
        // Parse only the one line; the full parse is the fallback for a
        // transcript whose malformed lines shift the numbering.
        const at = (raw: unknown) => imageAt({ line: raw, block, item });
        return (
          at(jsonlLineAt(text, line)) ?? at(parseJsonl(text)[line]) ?? null
        );
      }),
      Effect.withSpan("Sessions.image", { attributes: { id, line } })
    );

  return { list, parse, analyze: analyzeReq, image };
};

/** Live layer: depends on AgentRegistry + the platform FileSystem. */
export const SessionsServiceLive = Layer.effect(
  SessionsService,
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    const agents = yield* AgentRegistry;
    const cache = yield* makeHeaderCache();
    return makeService({ fs, agents, cache });
  })
);
